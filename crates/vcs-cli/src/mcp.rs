//! MCP server — speaks JSON-RPC 2.0 over stdio.
//!
//! Claude Code loads this via `.mcp.json`:
//!
//! ```json
//! { "mcpServers": { "vcs": { "command": "vcs", "args": ["mcp"] } } }
//! ```
//!
//! No Node.js required. The binary handles everything.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use vcs_core::{Intent, Resolution, Store};

// ── Tool catalogue ────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!([
      { "name": "vcs_status",
        "description": "Check if the vcs store is initialised and list open stacks from other sessions.",
        "inputSchema": { "type": "object", "properties": {
          "store_path": { "type": "string", "description": "Optional .vcs store path." }
        }}},
      { "name": "vcs_init",
        "description": "Initialise a new .vcs store in the given directory (default: current directory).",
        "inputSchema": { "type": "object", "properties": {
          "path": { "type": "string" }
        }}},
      { "name": "vcs_stack_open",
        "description": "Open a new stack for this agent session. Returns stack_id.",
        "inputSchema": { "type": "object", "required": ["agent_id"], "properties": {
          "agent_id":       { "type": "string" },
          "session_id":     { "type": "string" },
          "base_change_id": { "type": "string" },
          "store_path":     { "type": "string" }
        }}},
      { "name": "vcs_stack_close",
        "description": "Close a stack when the task is done.",
        "inputSchema": { "type": "object", "required": ["stack_id"], "properties": {
          "stack_id":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_stack_abandon",
        "description": "Abandon a stack on error or cancellation.",
        "inputSchema": { "type": "object", "required": ["stack_id"], "properties": {
          "stack_id":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_edit",
        "description": "Record a full-content edit directly in vcs. Prefer vcs_edit_from_disk for existing files.",
        "inputSchema": { "type": "object", "required": ["stack_id","path","content","reason"], "properties": {
          "stack_id":   { "type": "string" },
          "path":       { "type": "string" },
          "content":    { "type": "string" },
          "reason":     { "type": "string" },
          "task_ref":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_edit_from_disk",
        "description": "Agent-safe edit: reads the current file from disk as the base, seeds it into the stack if not already tracked, then records the new content. Never writes to disk. Use this as the default edit tool for existing files.",
        "inputSchema": { "type": "object", "required": ["stack_id","path","content","reason"], "properties": {
          "stack_id":   { "type": "string" },
          "path":       { "type": "string" },
          "content":    { "type": "string" },
          "reason":     { "type": "string" },
          "root_path":  { "type": "string", "description": "Project root for disk reads. Default: cwd." },
          "task_ref":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_delete",
        "description": "Record a file deletion in vcs.",
        "inputSchema": { "type": "object", "required": ["stack_id","path","reason"], "properties": {
          "stack_id":   { "type": "string" },
          "path":       { "type": "string" },
          "reason":     { "type": "string" },
          "task_ref":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_rename",
        "description": "Record a file rename/move in vcs.",
        "inputSchema": { "type": "object", "required": ["stack_id","from","to","content","reason"], "properties": {
          "stack_id":   { "type": "string" },
          "from":       { "type": "string" },
          "to":         { "type": "string" },
          "content":    { "type": "string" },
          "reason":     { "type": "string" },
          "task_ref":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_touching",
        "description": "Check which other open stacks are currently touching a file. Call after every vcs_edit to detect live collisions. Returns empty other_stacks when content is identical — no false positives.",
        "inputSchema": { "type": "object", "required": ["path","stack_id"], "properties": {
          "path":       { "type": "string" },
          "stack_id":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_view_open",
        "description": "Open a virtual merged view over one or more stacks.",
        "inputSchema": { "type": "object", "required": ["stack_ids"], "properties": {
          "stack_ids":      { "type": "array", "items": { "type": "string" } },
          "base_change_id": { "type": "string" },
          "store_path":     { "type": "string" }
        }}},
      { "name": "vcs_view_files",
        "description": "List all files tracked in a view.",
        "inputSchema": { "type": "object", "required": ["view_id"], "properties": {
          "view_id":    { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_view_read",
        "description": "Read a file's content from a view.",
        "inputSchema": { "type": "object", "required": ["view_id","path"], "properties": {
          "view_id":    { "type": "string" },
          "path":       { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_view_conflicts",
        "description": "List all unresolved conflicts in a view. Stop and report if non-empty — never resolve silently.",
        "inputSchema": { "type": "object", "required": ["view_id"], "properties": {
          "view_id":    { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_resolve",
        "description": "Resolve a conflict by picking one candidate or providing merged content.",
        "inputSchema": { "type": "object", "required": ["conflict_id"], "properties": {
          "conflict_id": { "type": "string" },
          "stack_id":    { "type": "string", "description": "Pick this stack's version." },
          "content":     { "type": "string", "description": "Custom merged content." },
          "store_path":  { "type": "string" }
        }}},
      { "name": "vcs_log",
        "description": "Show the change history for a stack (newest first).",
        "inputSchema": { "type": "object", "required": ["stack_id"], "properties": {
          "stack_id":   { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_history",
        "description": "Show complete change history across all stacks.",
        "inputSchema": { "type": "object", "properties": {
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_session_open",
        "description": "Register a new agent session. Call at the start of every task. Save the returned session_id.",
        "inputSchema": { "type": "object", "required": ["agent_id"], "properties": {
          "agent_id":   { "type": "string" },
          "port":       { "type": "integer" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_session_close",
        "description": "Deregister a session when the task is complete.",
        "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
          "session_id": { "type": "string" },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_session_phase",
        "description": "Update the session phase: working | testing | done.",
        "inputSchema": { "type": "object", "required": ["session_id","phase"], "properties": {
          "session_id": { "type": "string" },
          "phase":      { "type": "string", "enum": ["working","testing","done"] },
          "store_path": { "type": "string" }
        }}},
      { "name": "vcs_overview",
        "description": "Full multi-agent snapshot: sessions, hot files, conflicts. Call instead of asking the human to open a browser.",
        "inputSchema": { "type": "object", "properties": {
          "store_path": { "type": "string" }
        }}}
    ])
}

// ── Store helper ──────────────────────────────────────────────────────────

fn resolve_store(args: &Value, default: &Path) -> PathBuf {
    args.get("store_path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.to_path_buf())
}

fn open(store_path: &Path) -> Result<Store, String> {
    Store::open(store_path).map_err(|e| e.to_string())
}

fn safe_join(root: &Path, rel: &str) -> PathBuf {
    let clean = rel.replace('\\', "/");
    let mut out = root.to_path_buf();
    for seg in clean.split('/') {
        if seg == ".." || seg == "." || seg.is_empty() { continue; }
        out.push(seg);
    }
    out
}

// ── Tool dispatch ─────────────────────────────────────────────────────────

fn call_tool(name: &str, args: &Value, default_store: &Path) -> Result<Value, String> {
    let sp = resolve_store(args, default_store);
    let s  = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let os = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_owned);

    match name {
        "vcs_status" => {
            let db_path = sp.join("vcs.db");
            if !db_path.exists() {
                return Ok(json!({ "initialised": false,
                    "message": "Run vcs_init first." }));
            }
            let store = open(&sp)?;
            let open_stacks: Vec<_> = store.list_stacks()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|s| s.status == vcs_core::stack::StackStatus::Open)
                .collect();
            let count = open_stacks.len();
            let mut result = json!({ "initialised": true, "open_stacks": open_stacks });
            if count > 0 {
                result["warning"] = json!(format!(
                    "{count} stack(s) from other sessions are still OPEN. \
                     Check with vcs_overview before starting new work."
                ));
            }
            Ok(result)
        }

        "vcs_init" => {
            let init_path = os("path").map(PathBuf::from).unwrap_or_else(|| sp.clone());
            Store::init(&init_path).map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "path": init_path.display().to_string() }))
        }

        "vcs_stack_open" => {
            let agent_id = s("agent_id");
            if agent_id.is_empty() { return Err("agent_id required".into()); }
            let store = open(&sp)?;
            let base  = os("base_change_id");
            let stack_id = store.open_stack(&agent_id, base)
                .map_err(|e| e.to_string())?;
            // Link to session if provided
            if let Some(sid) = os("session_id") {
                let _ = store.session_link_stack(&sid, &stack_id);
            }
            Ok(json!({ "stack_id": stack_id }))
        }

        "vcs_stack_close" => {
            let store = open(&sp)?;
            store.close_stack(&s("stack_id")).map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "status": "closed" }))
        }

        "vcs_stack_abandon" => {
            let store = open(&sp)?;
            store.abandon_stack(&s("stack_id")).map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "status": "abandoned" }))
        }

        "vcs_edit" => {
            let store   = open(&sp)?;
            let content = s("content").into_bytes();
            let mut intent = Intent::new(&s("reason"));
            if let Some(tr) = os("task_ref") { intent = intent.with_task_ref(tr); }
            let change_id = store.edit(&s("stack_id"), &s("path"), &content, intent)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "change_id": change_id }))
        }

        "vcs_edit_from_disk" => {
            let store      = open(&sp)?;
            let path       = s("path");
            let new_content = s("content").into_bytes();
            let root       = os("root_path")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let disk_path  = safe_join(&root, &path);
            let stack_id   = s("stack_id");

            let snap = store.stack_snapshot(&stack_id).unwrap_or_default();
            let already_touched = snap.contains_key(&path);

            let seeded: Option<String> = if !already_touched && disk_path.exists() {
                let disk = std::fs::read(&disk_path).map_err(|e| e.to_string())?;
                if disk == new_content {
                    return Ok(json!({
                        "ok": true, "no_change": true,
                        "path": &path, "base_source": "disk",
                        "message": "content equals disk base; no vcs change recorded",
                    }));
                }
                let seed_intent = Intent::new(&format!("seed disk base for {path}"));
                Some(store.edit(&stack_id, &path, &disk, seed_intent)
                    .map_err(|e| e.to_string())?)
            } else { None };

            let mut intent = Intent::new(&s("reason"));
            if let Some(tr) = os("task_ref") { intent = intent.with_task_ref(tr); }
            let change_id = store.edit(&stack_id, &path, &new_content, intent)
                .map_err(|e| e.to_string())?;
            let base_source = if already_touched { "stack" }
                else if disk_path.exists() { "disk" } else { "new-file" };
            Ok(json!({
                "change_id": change_id,
                "path": &path,
                "base_source": base_source,
                "seeded_base_change_id": seeded,
            }))
        }

        "vcs_delete" => {
            let store = open(&sp)?;
            let mut intent = Intent::new(&s("reason"));
            if let Some(tr) = os("task_ref") { intent = intent.with_task_ref(tr); }
            let change_id = store.delete(&s("stack_id"), &s("path"), intent)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "change_id": change_id }))
        }

        "vcs_rename" => {
            let store   = open(&sp)?;
            let content = s("content").into_bytes();
            let mut intent = Intent::new(&s("reason"));
            if let Some(tr) = os("task_ref") { intent = intent.with_task_ref(tr); }
            let change_id = store.rename(&s("stack_id"), &s("from"), &s("to"), &content, intent)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "change_id": change_id }))
        }

        "vcs_touching" => {
            let store = open(&sp)?;
            let result = store.file_contention(&s("path"), &s("stack_id"))
                .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
        }

        "vcs_view_open" => {
            let store = open(&sp)?;
            let stack_ids: Vec<String> = args.get("stack_ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            let base = os("base_change_id").unwrap_or_default();
            let view_id = store.open_view(base, stack_ids.as_slice())
                .map_err(|e| e.to_string())?;
            Ok(json!({ "view_id": view_id }))
        }

        "vcs_view_files" => {
            let store = open(&sp)?;
            let files = store.list_files(&s("view_id"))
                .map_err(|e| e.to_string())?;
            Ok(json!({ "files": files }))
        }

        "vcs_view_read" => {
            let store = open(&sp)?;
            match store.read_file(&s("view_id"), &s("path")) {
                Ok(bytes) => Ok(json!({
                    "found": true,
                    "content": String::from_utf8_lossy(&bytes).to_string()
                })),
                Err(_) => Ok(json!({ "found": false })),
            }
        }

        "vcs_view_conflicts" => {
            let store = open(&sp)?;
            let conflicts = store.conflicts(&s("view_id"))
                .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&conflicts).map_err(|e| e.to_string())?)
        }

        "vcs_resolve" => {
            let store = open(&sp)?;
            let resolution = if let Some(content) = os("content") {
                // Store content in blob, use hash for Merge resolution
                let hash = store.put_blob(content.as_bytes())
                    .map_err(|e| e.to_string())?;
                Resolution::Merge { blob_hash: hash }
            } else if let Some(sid) = os("stack_id") {
                Resolution::Pick { stack_id: sid }
            } else {
                return Err("provide either stack_id (pick) or content (merge)".into());
            };
            store.resolve(&s("conflict_id"), resolution)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true }))
        }

        "vcs_log" => {
            let store = open(&sp)?;
            let log = store.log(&s("stack_id")).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&log).map_err(|e| e.to_string())?)
        }

        "vcs_history" => {
            let store = open(&sp)?;
            let hist = store.list_changes().map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&hist).map_err(|e| e.to_string())?)
        }

        "vcs_session_open" => {
            let store = open(&sp)?;
            let port = args.get("port").and_then(Value::as_u64).map(|p| p as u16);
            let session_id = store.session_open(&s("agent_id"), port)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "session_id": session_id }))
        }

        "vcs_session_close" => {
            let store = open(&sp)?;
            store.session_close(&s("session_id")).map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true }))
        }

        "vcs_session_phase" => {
            let store = open(&sp)?;
            store.session_set_phase(&s("session_id"), &s("phase"))
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true }))
        }

        "vcs_overview" => {
            let store = open(&sp)?;
            let overview = store.overview().map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&overview).map_err(|e| e.to_string())?)
        }

        _ => Err(format!("unknown tool: {name}")),
    }
}

// ── JSON-RPC helpers ──────────────────────────────────────────────────────

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_result(content: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": content }] })
}

// ── Main server loop ──────────────────────────────────────────────────────

pub fn run(default_store: &Path) -> Result<()> {
    let stdin  = std::io::stdin();
    let stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        let req: Value = match serde_json::from_str(&line) {
            Ok(v)  => v,
            Err(e) => {
                let resp = rpc_error(&Value::Null, -32700, &format!("parse error: {e}"));
                writeln!(stdout.lock(), "{}", serde_json::to_string(&resp)?)?;
                continue;
            }
        };

        let id     = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));

        let resp = match method {
            "initialize" => rpc_result(&id, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "vcs", "version": env!("CARGO_PKG_VERSION") }
            })),

            "initialized" | "notifications/initialized" => continue,

            "ping" => rpc_result(&id, json!({})),

            "tools/list" => rpc_result(&id, json!({ "tools": tool_list() })),

            "tools/call" => {
                let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let tool_args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call_tool(tool_name, &tool_args, default_store) {
                    Ok(result) => {
                        let text = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string());
                        rpc_result(&id, tool_result(&text))
                    }
                    Err(e) => rpc_result(&id, json!({
                        "content": [{ "type": "text", "text": e }],
                        "isError": true
                    })),
                }
            }

            _ => rpc_error(&id, -32601, &format!("method not found: {method}")),
        };

        writeln!(stdout.lock(), "{}", serde_json::to_string(&resp)?)?;
        stdout.lock().flush()?;
    }
    Ok(())
}
