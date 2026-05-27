//! vcs — thin CLI wrapper over vcs-core.
//!
//! All commands accept --json for machine-readable output.
//! Human output is plain text; JSON output is newline-terminated JSON.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;
use vcs_core::{Intent, Resolution, Store};

// ── Store path resolution ──────────────────────────────────────────────────

fn default_store_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".vcs-spike")
}

fn store_path(override_path: Option<&PathBuf>) -> PathBuf {
    override_path.cloned().unwrap_or_else(default_store_path)
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
    Log {
        stack_id: String,
    },

    /// Diff two change IDs
    Diff {
        from: String,
        to: String,
    },
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
    let sp = store_path(cli.store.as_ref());

    match cli.cmd {
        Cmd::Init => {
            let store = Store::init(&sp).context("init failed")?;
            drop(store);
            out(json, || println!("Initialised store at {}", sp.display()), || {
                json!({"ok": true, "path": sp.display().to_string()})
            });
        }

        Cmd::Stack(s) => match s {
            StackCmd::Open { agent, base } => {
                let store = open_store(&sp)?;
                let stack_id = store.open_stack(&agent, base).context("open_stack")?;
                out(json, || println!("{stack_id}"), || json!({"stack_id": stack_id}));
            }
            StackCmd::Close { stack_id } => {
                let store = open_store(&sp)?;
                store.close_stack(&stack_id).context("close_stack")?;
                out(json, || println!("closed {stack_id}"), || json!({"ok": true, "stack_id": stack_id}));
            }
            StackCmd::Abandon { stack_id } => {
                let store = open_store(&sp)?;
                store.abandon_stack(&stack_id).context("abandon_stack")?;
                out(json, || println!("abandoned {stack_id}"), || json!({"ok": true, "stack_id": stack_id}));
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
                        println!("base     : {}", stk.base_change_id.as_deref().unwrap_or("(root)"));
                        println!("tip      : {}", stk.tip_change_id.as_deref().unwrap_or("(empty)"));
                    },
                    || serde_json::to_value(&stk).unwrap(),
                );
            }
        },

        Cmd::Edit { stack_id, path, content_file, stdin, reason, task_ref, tool_call } => {
            let content = read_content(content_file, stdin)?;
            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref  { intent = intent.with_task_ref(tr); }
            if let Some(tc) = tool_call {
                let v: serde_json::Value = serde_json::from_str(&tc)?;
                intent = intent.with_tool_call(v);
            }
            let store = open_store(&sp)?;
            let change_id = store.edit(&stack_id, &path, &content, intent).context("edit")?;
            out(json, || println!("{change_id}"), || json!({"change_id": change_id}));
        }

        Cmd::Delete { stack_id, path, reason, task_ref } => {
            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref { intent = intent.with_task_ref(tr); }
            let store = open_store(&sp)?;
            let change_id = store.delete(&stack_id, &path, intent).context("delete")?;
            out(json, || println!("{change_id}"), || json!({"change_id": change_id}));
        }

        Cmd::Rename { stack_id, from, to, content_file, reason, task_ref } => {
            let content = read_content(content_file, false)?;
            let mut intent = Intent::new(&reason);
            if let Some(tr) = task_ref { intent = intent.with_task_ref(tr); }
            let store = open_store(&sp)?;
            let change_id = store.rename(&stack_id, &from, &to, &content, intent).context("rename")?;
            out(json, || println!("{change_id}"), || json!({"change_id": change_id}));
        }

        Cmd::View(v) => match v {
            ViewCmd::Open { base, stacks } => {
                let stack_ids: Vec<String> =
                    stacks.split(',').map(|s| s.trim().to_owned()).collect();
                let store = open_store(&sp)?;
                let view_id = store.open_view(base, &stack_ids).context("open_view")?;
                out(json, || println!("{view_id}"), || json!({"view_id": view_id}));
            }
            ViewCmd::Read { view_id, path } => {
                let store = open_store(&sp)?;
                let content = store.read_file(&view_id, &path).context("read_file")?;
                if json {
                    let s = String::from_utf8_lossy(&content);
                    println!("{}", serde_json::to_string(&json!({"path": path, "content": s})).unwrap());
                } else {
                    std::io::Write::write_all(&mut std::io::stdout(), &content)?;
                }
            }
            ViewCmd::Ls { view_id } => {
                let store = open_store(&sp)?;
                let files = store.list_files(&view_id).context("list_files")?;
                if json {
                    println!("{}", serde_json::to_string(&json!({"files": files})).unwrap());
                } else {
                    for f in &files { println!("{f}"); }
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
                            println!("  stack={} change={} blob={}",
                                cand.stack_id,
                                cand.change_id,
                                cand.blob_hash.as_deref().unwrap_or("(deleted)"));
                        }
                        println!("  resolution: {}", if c.resolution.is_some() { "resolved" } else { "UNRESOLVED" });
                    }
                }
            }
            ViewCmd::Resolve { conflict_id, pick, merge_blob, merge_file } => {
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
                out(json, || println!("resolved {conflict_id}"), || json!({"ok": true, "conflict_id": conflict_id}));
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
                    println!("{} | {} | {} | {}",
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
            let diff = store.diff(&from, &to).context("diff")?;
            if json {
                println!("{}", serde_json::to_string(&diff).unwrap());
            } else {
                for e in &diff {
                    let marker = match (&e.before_hash, &e.after_hash) {
                        (None, Some(_))    => "A",
                        (Some(_), None)    => "D",
                        (Some(_), Some(_)) => "M",
                        _ => "?",
                    };
                    println!("{} {}", marker, e.path);
                }
            }
        }
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
        None    => Ok(Vec::new()),
    }
}
