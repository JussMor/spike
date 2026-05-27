//! `vcs watch <dir> --stack <id>` — filesystem watcher for human devs.
//!
//! Watches a directory tree and auto-commits file changes to a vcs stack.
//! This is the **human dev UX** counterpart to agents calling `vcs edit` explicitly.
//! Point it at your project root while you edit files normally, and every save
//! is immediately committed to the vcs store.
//!
//! ```text
//! # Terminal A — watch while you work
//! $ vcs stack open --agent human-dev
//! abc12345
//! $ vcs watch . --stack abc12345
//! 👁  watching /home/alice/my-app → stack abc12345
//!     Ctrl+C → exit (stack stays open)
//!   M  src/App.tsx                              → 9f3c7a21ab12
//!   M  src/styles.css                           → 1b84c62de901
//!   D  src/old-component.tsx                    → a4501f38bb20
//!
//! # Terminal B — other agent sees changes immediately
//! $ vcs overview
//! ```
//!
//! ## Ignored paths
//!
//! By default: `.vcs/`, `.git/`, `node_modules/`, `target/`, `.next/`,
//! `dist/`, `.cache/`, `.turbo/`.  Add more with `--ignore <pattern>`.
//!
//! ## Event model
//!
//! | Disk event      | vcs operation             |
//! |-----------------|---------------------------|
//! | create / modify | `store.edit(stack, path, content)` |
//! | remove          | `store.delete(stack, path)` |
//! | rename          | delete old + edit new (two changes) |

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;
use vcs_core::{Intent, Store};

// ── Default ignore list ────────────────────────────────────────────────────

static DEFAULT_IGNORES: &[&str] = &[
    ".vcs",
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    ".cache",
    ".turbo",
    ".svn",
    ".hg",
];

// ── Public API ─────────────────────────────────────────────────────────────

pub struct WatchOptions {
    /// Stack to commit file changes into
    pub stack_id: String,
    /// Directory tree to watch (watched recursively)
    pub watch_dir: PathBuf,
    /// Additional top-level directory names to ignore (e.g. "build", ".output")
    pub extra_ignores: Vec<String>,
    /// If true, call `store.close_stack()` when Ctrl+C is pressed
    pub close_on_exit: bool,
    /// Milliseconds to wait after the first event before flushing (default: 50)
    pub debounce_ms: u64,
}

/// Run the watcher in a loop. Blocks until Ctrl+C.
pub fn run(store: Store, opts: WatchOptions) -> Result<()> {
    let watch_dir = opts
        .watch_dir
        .canonicalize()
        .unwrap_or_else(|_| opts.watch_dir.clone());

    // ── Ctrl+C handler ───────────────────────────────────────────────────────
    let running = Arc::new(AtomicBool::new(true));

    {
        let r = Arc::clone(&running);
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })?;
    }

    // ── Set up watcher ───────────────────────────────────────────────────────
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(&watch_dir, RecursiveMode::Recursive)?;

    println!(
        "👁  watching {} → stack {}",
        watch_dir.display(),
        &opts.stack_id[..8.min(opts.stack_id.len())]
    );
    if opts.close_on_exit {
        println!("    Ctrl+C → close stack and exit");
    } else {
        println!("    Ctrl+C → exit (stack stays open)");
    }
    println!();

    let debounce = Duration::from_millis(opts.debounce_ms);
    let poll_interval = Duration::from_millis(100);

    // ── Event loop ───────────────────────────────────────────────────────────
    while running.load(Ordering::SeqCst) {
        // Wait for the first event (short timeout so we check `running` regularly)
        let first = match rx.recv_timeout(poll_interval) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        // Drain all events that arrive within the debounce window
        let mut events = vec![first];
        loop {
            match rx.recv_timeout(debounce) {
                Ok(ev) => events.push(ev),
                _ => break,
            }
        }

        // Process the batch, deduplicating identical (kind, path) pairs
        let mut seen: HashSet<String> = HashSet::new();
        for ev_result in events {
            match ev_result {
                Ok(ev) => process_event(&store, &opts, &watch_dir, ev, &mut seen),
                Err(e) => eprintln!("[vcs-watch] watcher error: {e}"),
            }
        }
    }

    // ── Cleanup ──────────────────────────────────────────────────────────────
    if opts.close_on_exit {
        store.close_stack(&opts.stack_id)?;
        println!("\n[vcs-watch] stack {} closed — gate lifted", &opts.stack_id[..8.min(opts.stack_id.len())]);
    } else {
        println!("\n[vcs-watch] stopped — stack {} still open", &opts.stack_id[..8.min(opts.stack_id.len())]);
    }

    Ok(())
}

// ── Internal helpers ───────────────────────────────────────────────────────

fn process_event(
    store: &Store,
    opts: &WatchOptions,
    watch_dir: &Path,
    event: Event,
    seen: &mut HashSet<String>,
) {
    for abs_path in &event.paths {
        // Relative path (forward slashes, platform-neutral)
        let rel = match abs_path.strip_prefix(watch_dir) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        if rel.is_empty() {
            continue;
        }

        // Skip ignored top-level directories
        if should_ignore(&rel, &opts.extra_ignores) {
            continue;
        }

        // Skip directory-level events (we care about files only)
        if abs_path.is_dir() {
            continue;
        }

        // Deduplicate within this debounce window
        let key = format!("{:?}:{rel}", event.kind);
        if !seen.insert(key) {
            continue;
        }

        match &event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                // Race guard: file may have been removed between event and read
                if !abs_path.exists() {
                    continue;
                }
                match std::fs::read(abs_path) {
                    Ok(content) => {
                        let intent = Intent::new(&format!("watch: {rel} saved"));
                        match store.edit(&opts.stack_id, &rel, &content, intent) {
                            Ok(cid) => println!(
                                "  M  {rel:<44}  → {}",
                                &cid[..12.min(cid.len())]
                            ),
                            Err(e) => eprintln!("[vcs-watch] edit {rel}: {e}"),
                        }
                    }
                    Err(e) => eprintln!("[vcs-watch] read {}: {e}", abs_path.display()),
                }
            }

            EventKind::Remove(_) => {
                let intent = Intent::new(&format!("watch: {rel} deleted"));
                match store.delete(&opts.stack_id, &rel, intent) {
                    Ok(cid) => println!(
                        "  D  {rel:<44}  → {}",
                        &cid[..12.min(cid.len())]
                    ),
                    Err(e) => eprintln!("[vcs-watch] delete {rel}: {e}"),
                }
            }

            // Access, Other, Any, etc. — not tracked
            _ => {}
        }
    }
}

fn should_ignore(rel: &str, extra: &[String]) -> bool {
    let first = rel.split('/').next().unwrap_or("");
    if DEFAULT_IGNORES.contains(&first) {
        return true;
    }
    extra
        .iter()
        .any(|pat| first == pat.as_str() || rel.starts_with(pat.as_str()))
}
