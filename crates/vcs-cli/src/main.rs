//! vcs — thin CLI wrapper over vcs-core.
//!
//! All commands accept --json for machine-readable output.
//! Human output is plain text; JSON output is newline-terminated JSON.

mod serve;
mod watch;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::path::{Component, Path};
use vcs_core::{Intent, Resolution, Store};

// ── Store path resolution (git-like) ──────────────────────────────────────
//
// Priority order (same idea as git):
//   1. --store <path> flag
//   2. VCS_STORE_PATH env var
//   3. Walk CWD upward looking for a .vcs/ directory  ← like git finds .git/
//   4. ~/.vcs-spike/  (global fallback)

fn find_local_store() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".vcs");
        if candidate.join("vcs.db").exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

fn default_store_path() -> PathBuf {
    // Try local .vcs/ first
    if let Some(local) = find_local_store() {
        return local;
    }
    // Then env var
    if let Ok(p) = std::env::var("VCS_STORE_PATH") {
        return PathBuf::from(p);
    }
    // Global fallback
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".vcs-spike")
}

fn store_path(override_path: Option<&PathBuf>) -> PathBuf {
    if let Some(p) = override_path {
        return p.clone();
    }
    if let Ok(p) = std::env::var("VCS_STORE_PATH") {
        return PathBuf::from(p);
    }
    // Walk up looking for .vcs/
    if let Some(local) = find_local_store() {
        return local;
    }
    // Global fallback
    default_store_path()
}

// ── Output helpers ─────────────────────────────────────────────────────────

fn out(json_mode: bool, human: impl FnOnce(), machine: impl FnOnce() -> Value) {
    if json_mode {
        println!("{}", serde_json::to_string(&machine()).unwrap());
    } else {
        human();
    }
}

// ── CLI definition ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "vcs", about = "Agent-native VCS — spike edition", version)]
struct Cli {
    /// Path to the store directory (default: ~/.vcs-spike)
    #[arg(long, global = true)]
    store: Option<PathBuf>,

    /// Output JSON instead of human-readable text
    #[arg(long, short = 'j', global = true)]
    json: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Initialise a new store
    Init,

    /// Stack management
    #[command(subcommand)]
    Stack(StackCmd),

    /// Record a file edit
    Edit {
        /// Stack to record the edit in
        stack_id: String,
        /// File path inside the VCS namespace
        path: String,
        /// Read new content from this file
        #[arg(long)]
        content_file: Option<PathBuf>,
        /// Read new content from stdin (use - as path)
        #[arg(long)]
        stdin: bool,
        /// Reason for the edit (required)
        #[arg(long)]
        reason: String,
        /// Optional task reference
        #[arg(long)]
        task_ref: Option<String>,
        /// Optional tool-call JSON
        #[arg(long)]
        tool_call: Option<String>,
    },

    /// Agent-safe edit: reads the current disk file as the base, seeds it into the
    /// stack if not already present, then records new content.  Never writes disk.
    ///
    /// This is the recommended way for agents to record edits because:
    ///   1. The disk base is preserved → 3-way merge has full context
    ///   2. If content equals disk → no change recorded (idempotent)
    ///   3. If another agent already seeded this file → uses the stack base
    ///
    /// Use `vcs edit` only when you are creating a brand-new file that doesn't
    /// exist on disk yet.
    EditFromDisk {
        /// Stack to record the edit in
        stack_id: String,
        /// File path relative to the project root (also the disk path to seed from)
        path: String,
        /// Read new content from this file (must differ from the disk base)
        #[arg(long)]
        content_file: Option<PathBuf>,
        /// Read new content from stdin
        #[arg(long)]
        stdin: bool,
        /// Reason for the edit (required)
        #[arg(long)]
        reason: String,
        /// Project root for reading the disk base (default: CWD)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Optional task reference
        #[arg(long)]
        task_ref: Option<String>,
    },

    /// Record a file deletion
    Delete {
        stack_id: String,
        path: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        task_ref: Option<String>,
    },

    /// Record a rename
    Rename {
        stack_id: String,
        from: String,
        to: String,
        /// Read content at new path from this file
        #[arg(long)]
        content_file: Option<PathBuf>,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        task_ref: Option<String>,
    },

    /// View management
    #[command(subcommand)]
    View(ViewCmd),

    /// Show the change log for a stack
    Log { stack_id: String },

    /// Diff two change IDs
    Diff { from: String, to: String },

    /// Show complete change history across all stacks
    History,

    /// Materialize the file tree at a change ID into the working directory
    Checkout {
        change_id: String,
        /// Directory to write into (default: current directory)
        #[arg(long, default_value = ".")]
        worktree: PathBuf,
    },

    /// Manage remote stores
    #[command(subcommand)]
    Remote(RemoteCmd),

    /// Push this store's stacks, changes, and blobs to a remote vcs server
    Push {
        /// Remote name from config or direct http(s) URL
        remote: String,
        /// Project ID to include in the pushed bundle
        #[arg(long)]
        project_id: Option<String>,
    },

    /// Pull stacks, changes, and blobs from a remote vcs server
    Pull {
        /// Remote name from config or direct http(s) URL
        remote: String,
    },

    /// Start an HTTP hub server (enables multi-project communication)
    ///
    /// Agents in other projects push their stacks to this hub via:
    ///   POST http://host:<port>/api/vcs/push
    ///
    /// The hub builds a cross-project view and surfaces conflicts before
    /// anything is written to disk in any project.
    Serve {
        /// Port to listen on (default: 7474)
        #[arg(long, short, default_value = "7474")]
        port: u16,
        /// Require Authorization: Bearer <token> on push/pull endpoints.
        /// If omitted the hub accepts unauthenticated requests.
        #[arg(long)]
        token: Option<String>,
    },

    /// Session management (multi-session tracking)
    #[command(subcommand)]
    Session(SessionCmd),

    /// Show a full multi-agent overview — what every session is doing right now.
    ///
    /// Returns all active sessions, the files they are touching, and which files
    /// will conflict when stacks are merged.  Claude calls this instead of asking
    /// the human to open a browser.
    Overview,

    /// Check which other open stacks are currently touching a file.
    /// Returns immediately after an edit to warn about live collisions.
    Touching {
        /// File path to check
        path: String,
        /// The calling stack (excluded from results)
        #[arg(long)]
        stack: Option<String>,
    },

    /// Garbage-collect unreferenced blobs.
    ///
    /// Walks all non-abandoned stacks and their change chains, collects every
    /// referenced blob hash, then removes any blob file not in that set.
    /// Safe to run at any time — content-addressed blobs are never mutated.
    Gc,

    /// Watch a directory and auto-commit file saves to a stack (human dev UX).
    ///
    /// Every time you save a file, it is automatically committed to the given
    /// stack in the vcs store. Useful when you are editing files on disk and
    /// want changes tracked in real-time without calling `vcs edit` manually.
    ///
    /// Ignored by default: .vcs/ .git/ node_modules/ target/ .next/ dist/
    Watch {
        /// Stack to commit changes into
        #[arg(long)]
        stack: String,
        /// Directory to watch (default: current directory)
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Additional top-level directories to ignore (can repeat)
        #[arg(long = "ignore", value_name = "DIR")]
        ignores: Vec<String>,
        /// Close the stack when Ctrl+C is pressed (default: leave it open)
        #[arg(long)]
        close_on_exit: bool,
        /// Debounce window in milliseconds (default: 50)
        #[arg(long, default_value = "50")]
        debounce_ms: u64,
    },

    /// Manage API tokens for the hub server (multi-token ACL).
    ///
    /// Tokens are stored in <store>/tokens.json.  When at least one token is
    /// configured, `vcs serve` requires `Authorization: Bearer <token>` on all
    /// write endpoints.  Read-only tokens can call GET endpoints but not POST.
    #[command(subcommand)]
    Token(TokenCmd),
}

#[derive(Subcommand)]
enum StackCmd {
    /// Open a new stack
    Open {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        base: Option<String>,
    },
    /// Close a stack (work done)
    Close { stack_id: String },
    /// Abandon a stack
    Abandon { stack_id: String },
    /// Show stack info
    Info { stack_id: String },
    /// List all stacks (optionally filter by status)
    Ls {
        /// Only show stacks with this status: open | closed | abandoned
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    /// Register a new agent session (call at chat start)
    Open {
        #[arg(long)]
        agent: String,
        /// Reserve a port for this session's dev-server (recorded in overview)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Mark session done (call when task is complete)
    Close { session_id: String },
    /// Send a heartbeat to keep session alive
    Heartbeat { session_id: String },
    /// Link a stack to a session
    LinkStack { session_id: String, stack_id: String },
    /// Set the session phase: working | testing | done
    ///
    /// Phase=testing means this session is validating its output.
    /// No other session should merge until this session reaches done.
    Phase { session_id: String, phase: String },
    /// Record the output directory and port this session is serving from
    SetOutput {
        session_id: String,
        output_dir: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// List all sessions (newest first)
    Ls,
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// Add or update a named remote
    Add {
        name: String,
        url: String,
        /// Bearer token for Authorization header (stored in config.json)
        #[arg(long)]
        token: Option<String>,
    },
    /// Remove a named remote
    Remove { name: String },
    /// List configured remotes
    Ls,
}

#[derive(Subcommand)]
enum ViewCmd {
    /// Open a new view
    Open {
        #[arg(long)]
        base: String,
        /// Comma-separated stack IDs
        #[arg(long)]
        stacks: String,
    },
    /// Read a file through a view
    Read { view_id: String, path: String },
    /// List files in a view
    Ls { view_id: String },
    /// List conflicts in a view
    Conflicts { view_id: String },
    /// Resolve a conflict
    Resolve {
        conflict_id: String,
        /// Pick a stack: --pick <stack_id>
        #[arg(long, conflicts_with = "merge_blob")]
        pick: Option<String>,
        /// Merge: --merge-blob <blob_hash>
        #[arg(long)]
        merge_blob: Option<String>,
        /// Merge: provide merged content via file
        #[arg(long)]
        merge_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum TokenCmd {
    /// Add or update a named API token.
    ///
    /// The token value is stored in <store>/tokens.json alongside any remotes.
    /// Use `--read-only` to restrict this token to GET endpoints only.
    Add {
        /// Human-readable name for this token (e.g. "ci", "dashboard-viewer")
        name: String,
        /// The secret token value (sent as `Authorization: Bearer <value>`)
        value: String,
        /// Restrict to read-only operations (GET endpoints only)
        #[arg(long)]
        read_only: bool,
    },
    /// Remove a named token.
    Remove { name: String },
    /// List configured tokens (names and permissions — values are hidden).
    Ls,
}

// ── main ───────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vcs=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let json = cli.json;

    // For `init`, if no --store flag given, default to .vcs/ in CWD (git-like)
    let sp = if matches!(cli.cmd, Cmd::Init)
        && cli.store.is_none()
        && std::env::var("VCS_STORE_PATH").is_err()
    {
        std::env::current_dir()?.join(".vcs")
    } else {
        store_path(cli.store.as_ref())
    };

    match cli.cmd {
        Cmd::Init => {
            let store = Store::init(&sp).context("init failed")?;
            drop(store);
            out(
                json,
                || println!("Initialised store at {}", sp.display()),
                || json!({"ok": true, "path": sp.display().to_string()}),
            );
        }

        Cmd::Stack(s) => match s {
            StackCmd::Open { agent, base } => {
                let store = open_store(&sp)?;
                let stack_id = store.open_stack(&agent, base).context("open_stack")?;
                out(
                    json,
                    || println!("{stack_id}"),
                    || json!({"stack_id": stack_id}),
                );
            }
            StackCmd::Close { stack_id } => {
                let store = open_store(&sp)?;
                store.close_stack(&stack_id).context("close_stack")?;
                out(
                    json,
                    || println!("closed {stack_id}"),
                    || json!({"ok": true, "stack_id": stack_id}),
                );
            }
            StackCmd::Abandon { stack_id } => {
                let store = open_store(&sp)?;
                store.abandon_stack(&stack_id).context("abandon_stack")?;
                out(
                    json,
                    || println!("abandoned {stack_id}"),
                    || json!({"ok": true, "stack_id": stack_id}),
                );
            }
            StackCmd::Info { stack_id } => {
                let store = open_store(&sp)?;
                let stk = store.get_stack(&stack_id).context("get_stack")?;
                out(
                    json,
                    || {
                        println!("stack_id : {}", stk.stack_id);
                        println!("agent_id : {}", stk.agent_id);
                        println!("status   : {}", stk.status);
                        println!(
                            "base     : {}",
                            stk.base_change_id.as_deref().unwrap_or("(root)")
                        );
                        println!(
                            "tip      : {}",
                            stk.tip_change_id.as_deref().unwrap_or("(empty)")
                        );
                    },
                    || serde_json::to_value(&stk).unwrap(),
                );
            }
            StackCmd::Ls { status } => {
                let store = open_store(&sp)?;
                let all = store.list_stacks().context("list_stacks")?;
                let filtered: Vec<_> = match status.as_deref() {
                    Some(filter) => all
                        .into_iter()
                        .filter(|s| s.status.to_string() == filter)
                        .collect(),
                    None => all,
                };
                out(
                    json,
                    || {
                        if filtered.is_empty() {
                            println!("(no stacks)");
                        } else {
                            for s in &filtered {
                                println!(
                                    "{} | {} | {} | tip={}",
                                    &s.stack_id[..8],
                                    s.status,
                                    s.agent_id,
                                    s.tip_change_id.as_deref().unwrap_or("(empty)")
                                );
                            }
                        }
                    },
                    || serde_json::to_value(&filtered).unwrap(),
                );
            }
        },

        Cmd::Edit {
            stack_id,
            path,
            content_file,
            stdin,
            reason,
            task_ref,
            tool_call,
        } => {
            let content = read_content(content_file, stdin)?;
            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref {
                intent = intent.with_task_ref(tr);
            }
            if let Some(tc) = tool_call {
                let v: serde_json::Value = serde_json::from_str(&tc)?;
                intent = intent.with_tool_call(v);
            }
            let store = open_store(&sp)?;
            let change_id = store
                .edit(&stack_id, &path, &content, intent)
                .context("edit")?;
            out(
                json,
                || println!("{change_id}"),
                || json!({"change_id": change_id}),
            );
        }

        Cmd::EditFromDisk {
            stack_id,
            path,
            content_file,
            stdin,
            reason,
            root,
            task_ref,
        } => {
            let new_content = read_content(content_file, stdin)?;
            let root_dir = root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let disk_path = safe_join(&root_dir, &path)?;

            let store = open_store(&sp)?;

            // Check if this stack has already touched the file (has its own base)
            let snap = store.stack_snapshot(&stack_id).unwrap_or_default();
            let already_touched = snap.contains_key(&path);

            let seeded_change_id: Option<String> = if !already_touched && disk_path.exists() {
                let disk_content = std::fs::read(&disk_path)
                    .with_context(|| format!("reading disk base {}", disk_path.display()))?;
                // If the new content IS the disk content, nothing to record
                if disk_content == new_content {
                    out(
                        json,
                        || println!("no change (content equals disk base)"),
                        || json!({
                            "ok": true,
                            "no_change": true,
                            "path": &path,
                            "base_source": "disk",
                            "message": "content equals disk base; no vcs change recorded",
                        }),
                    );
                    return Ok(());
                }
                // Seed disk content as the base so 3-way merge has context
                let seed_intent = Intent::new(&format!("seed disk base for {path}"));
                let cid = store.edit(&stack_id, &path, &disk_content, seed_intent)
                    .context("seed disk base")?;
                Some(cid)
            } else {
                None
            };

            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref {
                intent = intent.with_task_ref(tr);
            }
            let change_id = store.edit(&stack_id, &path, &new_content, intent)
                .context("edit-from-disk")?;

            let base_source = if already_touched { "stack" }
                else if disk_path.exists() { "disk" }
                else { "new-file" };

            out(
                json,
                || println!("{change_id}"),
                || json!({
                    "change_id": change_id,
                    "path": &path,
                    "base_source": base_source,
                    "seeded_base_change_id": seeded_change_id,
                }),
            );
        }

        Cmd::Delete {
            stack_id,
            path,
            reason,
            task_ref,
        } => {
            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref {
                intent = intent.with_task_ref(tr);
            }
            let store = open_store(&sp)?;
            let change_id = store.delete(&stack_id, &path, intent).context("delete")?;
            out(
                json,
                || println!("{change_id}"),
                || json!({"change_id": change_id}),
            );
        }

        Cmd::Rename {
            stack_id,
            from,
            to,
            content_file,
            reason,
            task_ref,
        } => {
            let content = read_content(content_file, false)?;
            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref {
                intent = intent.with_task_ref(tr);
            }
            let store = open_store(&sp)?;
            let change_id = store
                .rename(&stack_id, &from, &to, &content, intent)
                .context("rename")?;
            out(
                json,
                || println!("{change_id}"),
                || json!({"change_id": change_id}),
            );
        }

        Cmd::View(v) => match v {
            ViewCmd::Open { base, stacks } => {
                let stack_ids: Vec<String> =
                    stacks.split(',').map(|s| s.trim().to_owned()).collect();
                let store = open_store(&sp)?;
                let view_id = store.open_view(base, &stack_ids).context("open_view")?;
                out(
                    json,
                    || println!("{view_id}"),
                    || json!({"view_id": view_id}),
                );
            }
            ViewCmd::Read { view_id, path } => {
                let store = open_store(&sp)?;
                let content = store.read_file(&view_id, &path).context("read_file")?;
                if json {
                    let s = String::from_utf8_lossy(&content);
                    println!(
                        "{}",
                        serde_json::to_string(&json!({"path": path, "content": s})).unwrap()
                    );
                } else {
                    std::io::Write::write_all(&mut std::io::stdout(), &content)?;
                }
            }
            ViewCmd::Ls { view_id } => {
                let store = open_store(&sp)?;
                let files = store.list_files(&view_id).context("list_files")?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string(&json!({"files": files})).unwrap()
                    );
                } else {
                    for f in &files {
                        println!("{f}");
                    }
                }
            }
            ViewCmd::Conflicts { view_id } => {
                let store = open_store(&sp)?;
                let conflicts = store.conflicts(&view_id).context("conflicts")?;
                if json {
                    println!("{}", serde_json::to_string(&conflicts).unwrap());
                } else if conflicts.is_empty() {
                    println!("no conflicts");
                } else {
                    for c in &conflicts {
                        println!("CONFLICT {} on {}", c.conflict_id, c.path);
                        for cand in &c.candidates {
                            println!(
                                "  stack={} change={} blob={}",
                                cand.stack_id,
                                cand.change_id,
                                cand.blob_hash.as_deref().unwrap_or("(deleted)")
                            );
                        }
                        println!(
                            "  resolution: {}",
                            if c.resolution.is_some() {
                                "resolved"
                            } else {
                                "UNRESOLVED"
                            }
                        );
                    }
                }
            }
            ViewCmd::Resolve {
                conflict_id,
                pick,
                merge_blob,
                merge_file,
            } => {
                let resolution = if let Some(sid) = pick {
                    Resolution::Pick { stack_id: sid }
                } else if let Some(bh) = merge_blob {
                    Resolution::Merge { blob_hash: bh }
                } else if let Some(mf) = merge_file {
                    let data = std::fs::read(&mf)?;
                    let store = open_store(&sp)?;
                    let hash = store.put_blob(&data)?;
                    Resolution::Merge { blob_hash: hash }
                } else {
                    anyhow::bail!("provide one of --pick, --merge-blob, or --merge-file");
                };

                let store = open_store(&sp)?;
                store.resolve(&conflict_id, resolution).context("resolve")?;
                out(
                    json,
                    || println!("resolved {conflict_id}"),
                    || json!({"ok": true, "conflict_id": conflict_id}),
                );
            }
        },

        Cmd::Log { stack_id } => {
            let store = open_store(&sp)?;
            let log = store.log(&stack_id).context("log")?;
            if json {
                println!("{}", serde_json::to_string(&log).unwrap());
            } else if log.is_empty() {
                println!("(empty stack)");
            } else {
                for c in &log {
                    println!(
                        "{} | {} | {} | {}",
                        &c.change_id[..12],
                        c.op,
                        c.path,
                        c.intent.reason,
                    );
                }
            }
        }

        Cmd::Diff { from, to } => {
            let store = open_store(&sp)?;
            // diff_chain walks the chain from `to` back to `from`, returning
            // every path touched with op=create|edit|delete (most-recent-wins).
            // This is what the vcs-vite HMR poller needs for targeted invalidation.
            let diff = store.diff_chain(&from, &to).context("diff")?;
            if json {
                println!("{}", serde_json::to_string(&diff).unwrap());
            } else {
                for e in &diff {
                    let marker = match e.op.as_str() {
                        "create" => "A",
                        "delete" => "D",
                        _        => "M",
                    };
                    println!("{} {}", marker, e.path);
                }
            }
        }

        Cmd::History => {
            let store = open_store(&sp)?;
            let changes = store.list_changes().context("history")?;
            if json {
                println!("{}", serde_json::to_string(&changes).unwrap());
            } else if changes.is_empty() {
                println!("(empty history)");
            } else {
                for c in &changes {
                    println!(
                        "{} | {} | {} | {} | {}",
                        &c.change_id[..12],
                        c.created_at,
                        c.op,
                        c.path,
                        c.intent.reason,
                    );
                }
            }
        }

        Cmd::Checkout {
            change_id,
            worktree,
        } => {
            let store = open_store(&sp)?;
            let written = checkout_change(&store, &change_id, &worktree)
                .with_context(|| format!("checkout {change_id}"))?;
            out(
                json,
                || {
                    println!("checked out {change_id} to {}", worktree.display());
                    println!("  wrote:   {}", written.written);
                    println!("  removed: {}", written.removed);
                },
                || {
                    json!({
                        "ok": true,
                        "change_id": change_id,
                        "worktree": worktree.display().to_string(),
                        "written": written.written,
                        "removed": written.removed,
                    })
                },
            );
        }

        Cmd::Remote(r) => match r {
            RemoteCmd::Add { name, url, token } => {
                let mut config = RemoteConfig::load(&sp)?;
                let entry = RemoteEntry {
                    url: normalize_remote_url(&url),
                    token,
                };
                config.remotes.insert(name.clone(), entry);
                config.save(&sp)?;
                let display_url = &config.remotes[&name].url;
                out(
                    json,
                    || println!("{name} {display_url}"),
                    || json!({"ok": true, "name": name, "url": display_url}),
                );
            }
            RemoteCmd::Remove { name } => {
                let mut config = RemoteConfig::load(&sp)?;
                let removed = config.remotes.remove(&name).is_some();
                config.save(&sp)?;
                out(
                    json,
                    || {
                        if removed {
                            println!("removed {name}");
                        } else {
                            println!("{name} not configured");
                        }
                    },
                    || json!({"ok": removed, "name": name}),
                );
            }
            RemoteCmd::Ls => {
                let config = RemoteConfig::load(&sp)?;
                if json {
                    println!("{}", serde_json::to_string(&config).unwrap());
                } else {
                    for (name, entry) in &config.remotes {
                        let auth = if entry.token.is_some() { " [auth]" } else { "" };
                        println!("{name}\t{}{auth}", entry.url);
                    }
                }
            }
        },

        Cmd::Push { remote, project_id } => {
            let store = open_store(&sp)?;
            let entry = resolve_remote_entry(&sp, &remote)?;
            let url = &entry.url;
            let project_id = project_id.unwrap_or_else(|| project_id_from_cwd());
            let bundle = store.export_bundle(&project_id).context("export bundle")?;
            let stacks = bundle.stacks.len();
            let changes = bundle.changes.len();
            let blobs = bundle.blobs.len();
            let endpoint = format!("{}/api/vcs/push", url);
            let mut req = ureq::post(&endpoint);
            if let Some(ref tok) = entry.token {
                req = req.set("Authorization", &format!("Bearer {tok}"));
            }
            let response: Value = req
                .send_json(serde_json::to_value(&bundle)?)
                .with_context(|| format!("POST {endpoint}"))?
                .into_json()
                .with_context(|| format!("decode response from {endpoint}"))?;
            out(
                json,
                || {
                    println!("pushed {project_id} to {url}");
                    println!("  stacks:  {stacks}");
                    println!("  changes: {changes}");
                    println!("  blobs:   {blobs}");
                },
                || {
                    json!({
                        "ok": true,
                        "remote": url,
                        "project_id": project_id,
                        "stacks": stacks,
                        "changes": changes,
                        "blobs": blobs,
                        "response": response,
                    })
                },
            );
        }

        Cmd::Pull { remote } => {
            let store = open_store(&sp)?;
            let entry = resolve_remote_entry(&sp, &remote)?;
            let url = &entry.url;
            let endpoint = format!("{}/api/vcs/export", url);
            let mut req = ureq::get(&endpoint);
            if let Some(ref tok) = entry.token {
                req = req.set("Authorization", &format!("Bearer {tok}"));
            }
            let bundle: vcs_core::HubBundle = req
                .call()
                .with_context(|| format!("GET {endpoint}"))?
                .into_json()
                .with_context(|| format!("decode bundle from {endpoint}"))?;
            let project_id = bundle.project_id.clone();
            let (blobs, stacks, changes) = store.import_bundle(&bundle).context("import bundle")?;
            out(
                json,
                || {
                    println!("pulled {project_id} from {url}");
                    println!("  stacks:  {stacks}");
                    println!("  changes: {changes}");
                    println!("  blobs:   {blobs}");
                },
                || {
                    json!({
                        "ok": true,
                        "remote": url,
                        "project_id": project_id,
                        "stacks": stacks,
                        "changes": changes,
                        "blobs": blobs,
                    })
                },
            );
        }

        Cmd::Serve { port, token } => {
            // Ensure the hub store exists
            let store = Store::open_or_init(&sp).context("opening hub store")?;
            println!("vcs hub store: {}", sp.display());

            // Build token list: --token flag (write-capable) + tokens.json entries
            let mut serve_tokens: Vec<serve::ServeToken> = Vec::new();
            if let Some(t) = token {
                serve_tokens.push(serve::ServeToken { value: t, read_only: false });
            }
            let tc = TokenConfig::load(&sp).unwrap_or_default();
            serve_tokens.extend(tc.into_serve_tokens());

            if !serve_tokens.is_empty() {
                let rw = serve_tokens.iter().filter(|t| !t.read_only).count();
                let ro = serve_tokens.iter().filter(|t|  t.read_only).count();
                println!("vcs hub auth: {} write token(s), {} read-only token(s)", rw, ro);
            }

            // Spin up a tokio runtime only for serve (keeps other commands synchronous)
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::run(store, port, serve_tokens))?;
        }

        Cmd::Session(s) => match s {
            SessionCmd::Open { agent, port } => {
                let store = open_store(&sp)?;
                let sid = store.session_open(&agent, port).context("session_open")?;
                out(json, || println!("{sid}"), || json!({"session_id": sid}));
            }
            SessionCmd::Close { session_id } => {
                let store = open_store(&sp)?;
                store.session_close(&session_id).context("session_close")?;
                out(json, || println!("closed {session_id}"),
                    || json!({"ok": true, "session_id": session_id}));
            }
            SessionCmd::Heartbeat { session_id } => {
                let store = open_store(&sp)?;
                store.session_heartbeat(&session_id).context("heartbeat")?;
                out(json, || {}, || json!({"ok": true}));
            }
            SessionCmd::LinkStack { session_id, stack_id } => {
                let store = open_store(&sp)?;
                store.session_link_stack(&session_id, &stack_id).context("link_stack")?;
                out(json, || println!("linked {stack_id} → session {session_id}"),
                    || json!({"ok": true}));
            }
            SessionCmd::Phase { session_id, phase } => {
                let store = open_store(&sp)?;
                store.session_set_phase(&session_id, &phase).context("set_phase")?;
                out(json,
                    || println!("session {session_id} → phase={phase}"),
                    || json!({"ok": true, "session_id": session_id, "phase": phase}));
            }
            SessionCmd::SetOutput { session_id, output_dir, port } => {
                let store = open_store(&sp)?;
                store.session_set_output(&session_id, &output_dir, port).context("set_output")?;
                out(json,
                    || println!("session {session_id} → output={output_dir}"),
                    || json!({"ok": true}));
            }
            SessionCmd::Ls => {
                let store = open_store(&sp)?;
                let sessions = store.list_sessions().context("list_sessions")?;
                out(json,
                    || {
                        if sessions.is_empty() {
                            println!("(no sessions)");
                        } else {
                            for s in &sessions {
                                let port_str = s.port.map(|p| format!(" ::{p}")).unwrap_or_default();
                                println!("{} | {} | phase={} | {} | stack={}{port_str}",
                                    &s.session_id[..8],
                                    s.status,
                                    s.phase,
                                    s.agent_id,
                                    s.stack_id.as_deref().unwrap_or("(none)"),
                                );
                            }
                        }
                    },
                    || serde_json::to_value(&sessions).unwrap(),
                );
            }
        },

        Cmd::Overview => {
            let store = open_store(&sp)?;
            let ov = store.overview().context("overview")?;
            out(json,
                || {
                    println!("{}", ov.summary);
                    if !ov.hot_files.is_empty() {
                        println!("\n⚡ Files that WILL conflict:");
                        for hf in &ov.hot_files {
                            println!("  {} ← {}", hf.path, hf.touched_by.join(", "));
                        }
                    }
                },
                || serde_json::to_value(&ov).unwrap(),
            );
        }

        Cmd::Touching { path, stack } => {
            let store = open_store(&sp)?;
            let caller_stack = stack.as_deref().unwrap_or("__none__");
            let contention = store.file_contention(&path, caller_stack)
                .context("file_contention")?;
            out(json,
                || {
                    if contention.other_stacks.is_empty() {
                        println!("no contention on {path}");
                    } else {
                        println!("⚡ {} other open stack(s) are touching {}:",
                            contention.other_stacks.len(), path);
                        for e in &contention.other_stacks {
                            println!("  {} ({})", e.agent_id, &e.stack_id[..8]);
                        }
                    }
                },
                || serde_json::to_value(&contention).unwrap(),
            );
        }

        Cmd::Gc => {
            let store = open_store(&sp)?;
            let freed = store.gc().context("gc")?;
            out(
                json,
                || println!("freed {freed} blob(s)"),
                || json!({"ok": true, "freed_blobs": freed}),
            );
        }

        Cmd::Watch {
            stack,
            dir,
            ignores,
            close_on_exit,
            debounce_ms,
        } => {
            let store = open_store(&sp)?;
            let opts = watch::WatchOptions {
                stack_id:      stack,
                watch_dir:     dir,
                extra_ignores: ignores,
                close_on_exit,
                debounce_ms,
            };
            watch::run(store, opts)?;
        }

        Cmd::Token(t) => match t {
            TokenCmd::Add { name, value, read_only } => {
                let mut config = TokenConfig::load(&sp)?;
                config.tokens.insert(name.clone(), TokenEntry { token: value, read_only });
                config.save(&sp)?;
                let ro = if read_only { " (read-only)" } else { "" };
                out(
                    json,
                    || println!("token '{name}' saved{ro}"),
                    || json!({"ok": true, "name": name, "read_only": read_only}),
                );
            }
            TokenCmd::Remove { name } => {
                let mut config = TokenConfig::load(&sp)?;
                let removed = config.tokens.remove(&name).is_some();
                config.save(&sp)?;
                out(
                    json,
                    || {
                        if removed { println!("removed token '{name}'"); }
                        else       { println!("token '{name}' not found"); }
                    },
                    || json!({"ok": removed, "name": name}),
                );
            }
            TokenCmd::Ls => {
                let config = TokenConfig::load(&sp)?;
                if json {
                    // Never expose token values in JSON output
                    let safe: BTreeMap<&str, serde_json::Value> = config
                        .tokens
                        .iter()
                        .map(|(k, e)| (k.as_str(), json!({"read_only": e.read_only})))
                        .collect();
                    println!("{}", serde_json::to_string(&safe).unwrap());
                } else if config.tokens.is_empty() {
                    println!("(no tokens configured)");
                } else {
                    for (name, entry) in &config.tokens {
                        let ro = if entry.read_only { "  [read-only]" } else { "  [read-write]" };
                        println!("{name}{ro}");
                    }
                }
            }
        },
    }

    Ok(())
}

// ── helpers ────────────────────────────────────────────────────────────────

fn open_store(path: &std::path::Path) -> Result<Store> {
    Store::open_or_init(path).with_context(|| format!("opening store at {}", path.display()))
}

fn read_content(file: Option<PathBuf>, stdin: bool) -> Result<Vec<u8>> {
    if stdin {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        return Ok(buf);
    }
    match file {
        Some(p) => Ok(std::fs::read(&p)?),
        None => Ok(Vec::new()),
    }
}

/// A remote entry — URL plus optional Bearer token.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteEntry {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RemoteConfig {
    #[serde(default)]
    remotes: BTreeMap<String, RemoteEntry>,
}

impl RemoteConfig {
    fn load(store_path: &Path) -> Result<Self> {
        let path = remote_config_path(store_path);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        // Try new format first; fall back to old string-map format for migration.
        if let Ok(cfg) = serde_json::from_str::<Self>(&data) {
            return Ok(cfg);
        }
        // Old format: { "remotes": { "name": "url" } }
        #[derive(Deserialize)]
        struct OldRemoteConfig {
            #[serde(default)]
            remotes: BTreeMap<String, String>,
        }
        if let Ok(old) = serde_json::from_str::<OldRemoteConfig>(&data) {
            return Ok(RemoteConfig {
                remotes: old.remotes.into_iter().map(|(k, url)| {
                    (k, RemoteEntry { url, token: None })
                }).collect(),
            });
        }
        anyhow::bail!("could not parse {}", path.display())
    }

    fn save(&self, store_path: &Path) -> Result<()> {
        std::fs::create_dir_all(store_path)?;
        let path = remote_config_path(store_path);
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, format!("{data}\n"))
            .with_context(|| format!("writing {}", path.display()))
    }
}

fn remote_config_path(store_path: &Path) -> PathBuf {
    store_path.join("config.json")
}

// ── Token config ───────────────────────────────────────────────────────────

/// A single named API token stored in `tokens.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenEntry {
    token: String,
    #[serde(default)]
    read_only: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokenConfig {
    #[serde(default)]
    tokens: BTreeMap<String, TokenEntry>,
}

impl TokenConfig {
    fn load(store_path: &Path) -> Result<Self> {
        let path = store_path.join("tokens.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Ok(serde_json::from_str(&data)?)
    }

    fn save(&self, store_path: &Path) -> Result<()> {
        std::fs::create_dir_all(store_path)?;
        let path = store_path.join("tokens.json");
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, format!("{data}\n"))
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Convert to the flat list that `serve::run()` expects.
    fn into_serve_tokens(self) -> Vec<serve::ServeToken> {
        self.tokens
            .into_values()
            .map(|e| serve::ServeToken { value: e.token, read_only: e.read_only })
            .collect()
    }
}

/// Resolve a remote name or URL to a `RemoteEntry`.
fn resolve_remote_entry(store_path: &Path, remote: &str) -> Result<RemoteEntry> {
    if remote.starts_with("http://") || remote.starts_with("https://") {
        return Ok(RemoteEntry { url: normalize_remote_url(remote), token: None });
    }
    let config = RemoteConfig::load(store_path)?;
    config
        .remotes
        .get(remote)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("remote not configured: {remote}"))
}

fn normalize_remote_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

fn project_id_from_cwd() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "project".to_owned())
}

struct CheckoutStats {
    written: usize,
    removed: usize,
}

fn checkout_change(store: &Store, change_id: &str, worktree: &Path) -> Result<CheckoutStats> {
    let snapshot = store.snapshot_at(change_id)?;
    let snapshot_paths: HashSet<&str> = snapshot.keys().map(String::as_str).collect();

    let mut removed = 0usize;
    for path in store.list_tracked_paths()? {
        if snapshot_paths.contains(path.as_str()) {
            continue;
        }
        let target = safe_join(worktree, &path)?;
        if target.exists() {
            std::fs::remove_file(&target)
                .with_context(|| format!("removing {}", target.display()))?;
            removed += 1;
        }
    }

    let mut written = 0usize;
    for (path, hash) in snapshot {
        let target = safe_join(worktree, &path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let data = store
            .get_blob(&hash)
            .with_context(|| format!("reading blob {hash} for {path}"))?;
        std::fs::write(&target, data).with_context(|| format!("writing {}", target.display()))?;
        written += 1;
    }

    Ok(CheckoutStats { written, removed })
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute() {
        anyhow::bail!("refusing to checkout absolute path: {relative}");
    }

    let mut out = root.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                anyhow::bail!("refusing to checkout path outside worktree: {relative}");
            }
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("refusing to checkout invalid path: {relative}");
            }
        }
    }
    Ok(out)
}
