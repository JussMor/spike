//! M2 — single-agent happy path.

use tempfile::TempDir;
use vcs_core::{Intent, Store};

fn store() -> (Store, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::init(dir.path()).unwrap();
    (s, dir)
}

#[test]
fn edit_and_read_through_view() {
    let (store, _dir) = store();

    // Agent opens a stack
    let stack = store.open_stack("agent-1", None).unwrap();

    // Write a root snapshot so the view has something to stand on
    let base_hash = store.put_blob(b"").unwrap(); // empty sentinel

    // Edit three files
    store
        .edit(&stack, "src/main.rs", b"fn main() {}", Intent::new("initial main"))
        .unwrap();
    store
        .edit(&stack, "src/lib.rs", b"pub fn hello() {}", Intent::new("add lib"))
        .unwrap();
    let tip = store
        .edit(&stack, "README.md", b"# project", Intent::new("add readme"))
        .unwrap();

    store.close_stack(&stack).unwrap();

    // Open a view from the tip of this stack
    let view = store.open_view(tip.clone(), &[stack.clone()]).unwrap();

    let main_rs = store.read_file(&view, "src/main.rs").unwrap();
    assert_eq!(main_rs, b"fn main() {}");

    let lib_rs = store.read_file(&view, "src/lib.rs").unwrap();
    assert_eq!(lib_rs, b"pub fn hello() {}");

    let readme = store.read_file(&view, "README.md").unwrap();
    assert_eq!(readme, b"# project");

    // List files
    let files = store.list_files(&view).unwrap();
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"src/lib.rs".to_string()));
    assert!(files.contains(&"README.md".to_string()));
}

#[test]
fn log_returns_changes_oldest_first() {
    let (store, _dir) = store();
    let stack = store.open_stack("agent-log", None).unwrap();

    store.edit(&stack, "a.txt", b"aaa", Intent::new("first")).unwrap();
    store.edit(&stack, "b.txt", b"bbb", Intent::new("second")).unwrap();
    store.edit(&stack, "c.txt", b"ccc", Intent::new("third")).unwrap();

    let log = store.log(&stack).unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].intent.reason, "first");
    assert_eq!(log[2].intent.reason, "third");
}

#[test]
fn delete_removes_file_from_view() {
    let (store, _dir) = store();
    let stack = store.open_stack("agent-del", None).unwrap();

    store.edit(&stack, "file.txt", b"hello", Intent::new("create")).unwrap();
    let tip = store.delete(&stack, "file.txt", Intent::new("remove it")).unwrap();

    let view = store.open_view(tip.clone(), &[stack.clone()]).unwrap();
    let files = store.list_files(&view).unwrap();
    assert!(!files.contains(&"file.txt".to_string()));
}

#[test]
fn rename_old_gone_new_present() {
    let (store, _dir) = store();
    let stack = store.open_stack("agent-rename", None).unwrap();

    store.edit(&stack, "old.txt", b"content", Intent::new("create")).unwrap();
    let tip = store
        .rename(&stack, "old.txt", "new.txt", b"content", Intent::new("rename"))
        .unwrap();

    let view = store.open_view(tip.clone(), &[stack.clone()]).unwrap();
    let files = store.list_files(&view).unwrap();
    assert!(!files.contains(&"old.txt".to_string()), "old path should be gone");
    assert!(files.contains(&"new.txt".to_string()), "new path should exist");

    let content = store.read_file(&view, "new.txt").unwrap();
    assert_eq!(content, b"content");
}

#[test]
fn intent_survives_round_trip() {
    use serde_json::json;
    let (store, _dir) = store();
    let stack = store.open_stack("agent-intent", None).unwrap();

    let intent = Intent::new("refactor main loop")
        .with_tool_call(json!({"name": "edit_file", "args": {"path": "src/main.rs"}}))
        .with_task_ref("task-42");

    store.edit(&stack, "src/main.rs", b"fn main() {}", intent).unwrap();

    let log = store.log(&stack).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].intent.reason, "refactor main loop");
    assert!(log[0].intent.tool_call.is_some());
    assert_eq!(log[0].intent.task_ref.as_deref(), Some("task-42"));
}
