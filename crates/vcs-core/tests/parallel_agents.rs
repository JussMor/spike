//! M3 — parallel agents editing non-overlapping files.

use tempfile::TempDir;
use vcs_core::{Intent, Store};

fn store() -> (Store, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Store::init(dir.path()).unwrap(), dir)
}

#[test]
fn two_agents_no_overlap_merge_cleanly() {
    let (store, _dir) = store();

    // Seed a base file so agents have a common ancestor
    let base_stack = store.open_stack("seed", None).unwrap();
    let base_tip = store
        .edit(&base_stack, "shared.txt", b"shared content", Intent::new("seed"))
        .unwrap();
    store.close_stack(&base_stack).unwrap();

    // Agent A works on api.rs, branching from the base tip
    let stack_a = store.open_stack("agent-a", Some(base_tip.clone())).unwrap();
    store
        .edit(&stack_a, "src/api.rs", b"pub fn api() {}", Intent::new("add API"))
        .unwrap();
    let tip_a = store
        .edit(&stack_a, "src/api.rs", b"pub fn api() -> i32 { 42 }", Intent::new("fix return"))
        .unwrap();
    store.close_stack(&stack_a).unwrap();

    // Agent B works on worker.rs, branching from the same base tip
    let stack_b = store.open_stack("agent-b", Some(base_tip.clone())).unwrap();
    let tip_b = store
        .edit(&stack_b, "src/worker.rs", b"pub fn work() {}", Intent::new("add worker"))
        .unwrap();
    store.close_stack(&stack_b).unwrap();

    // Open a view with both stacks
    let view = store
        .open_view(base_tip.clone(), &[stack_a.clone(), stack_b.clone()])
        .unwrap();

    // No conflicts
    let conflicts = store.conflicts(&view).unwrap();
    assert!(conflicts.is_empty(), "expected no conflicts, got: {conflicts:?}");

    // Both files visible
    let api = store.read_file(&view, "src/api.rs").unwrap();
    assert_eq!(api, b"pub fn api() -> i32 { 42 }");

    let worker = store.read_file(&view, "src/worker.rs").unwrap();
    assert_eq!(worker, b"pub fn work() {}");

    // Inherited base file still visible
    let shared = store.read_file(&view, "shared.txt").unwrap();
    assert_eq!(shared, b"shared content");

    let files = store.list_files(&view).unwrap();
    assert_eq!(files.len(), 3);
}

#[test]
fn four_agents_no_overlap() {
    let (store, _dir) = store();

    // Seed
    let seed = store.open_stack("seed", None).unwrap();
    let base = store
        .edit(&seed, "base.txt", b"root", Intent::new("root"))
        .unwrap();
    store.close_stack(&seed).unwrap();

    let files = ["a.rs", "b.rs", "c.rs", "d.rs"];
    let mut stacks = Vec::new();

    for (i, f) in files.iter().enumerate() {
        let s = store
            .open_stack(&format!("agent-{i}"), Some(base.clone()))
            .unwrap();
        store
            .edit(&s, f, format!("content of {f}").as_bytes(), Intent::new(format!("write {f}")))
            .unwrap();
        store.close_stack(&s).unwrap();
        stacks.push(s);
    }

    let view = store.open_view(base.clone(), &stacks).unwrap();
    let conflicts = store.conflicts(&view).unwrap();
    assert!(conflicts.is_empty());

    let listed = store.list_files(&view).unwrap();
    // base.txt + 4 agent files
    assert_eq!(listed.len(), 5);
}
