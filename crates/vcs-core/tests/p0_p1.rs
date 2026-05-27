//! Tests for P0.1 diff_chain, P0.2 content-aware conflicts,
//! P1.2 base-aware 3-way merge, and P1.3 blob GC.

use tempfile::TempDir;
use vcs_core::{Intent, Store};

fn store() -> (Store, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::init(dir.path()).unwrap();
    (s, dir)
}

// ── P0.1 diff_chain ────────────────────────────────────────────────────────

#[test]
fn diff_chain_single_edit() {
    let (store, _dir) = store();
    let stack = store.open_stack("a", None).unwrap();
    let c1 = store.edit(&stack, "src/a.ts", b"v1", Intent::new("init")).unwrap();
    let c2 = store.edit(&stack, "src/a.ts", b"v2", Intent::new("update")).unwrap();

    let diff = store.diff_chain(&c1, &c2).unwrap();
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].path, "src/a.ts");
    assert_eq!(diff[0].op, "edit");
}

#[test]
fn diff_chain_create_and_delete() {
    let (store, _dir) = store();
    let stack = store.open_stack("a", None).unwrap();
    let c1 = store.edit(&stack, "src/a.ts", b"hello", Intent::new("create")).unwrap();
    let c2 = store.delete(&stack, "src/a.ts", Intent::new("remove")).unwrap();

    // diff from c1 to c2 — file deleted after c1
    let diff = store.diff_chain(&c1, &c2).unwrap();
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].op, "delete");
}

#[test]
fn diff_chain_from_eq_to_is_empty() {
    let (store, _dir) = store();
    let stack = store.open_stack("a", None).unwrap();
    let c1 = store.edit(&stack, "src/a.ts", b"v1", Intent::new("x")).unwrap();
    let diff = store.diff_chain(&c1, &c1).unwrap();
    assert!(diff.is_empty());
}

#[test]
fn diff_chain_multiple_files_most_recent_wins() {
    let (store, _dir) = store();
    let stack = store.open_stack("a", None).unwrap();
    let c1 = store.edit(&stack, "src/a.ts", b"v1", Intent::new("x")).unwrap();
    // Two edits to same file — only the most recent should appear
    let _c2 = store.edit(&stack, "src/a.ts", b"v2", Intent::new("x")).unwrap();
    let c3 = store.edit(&stack, "src/a.ts", b"v3", Intent::new("x")).unwrap();

    let diff = store.diff_chain(&c1, &c3).unwrap();
    // Only one entry for src/a.ts despite two changes
    let a_entries: Vec<_> = diff.iter().filter(|e| e.path == "src/a.ts").collect();
    assert_eq!(a_entries.len(), 1);
    assert_eq!(a_entries[0].op, "edit");
}

#[test]
fn diff_chain_from_empty_shows_creates() {
    let (store, _dir) = store();
    let stack = store.open_stack("a", None).unwrap();
    let _c1 = store.edit(&stack, "src/a.ts", b"v1", Intent::new("x")).unwrap();
    let c2 = store.edit(&stack, "src/b.ts", b"v1", Intent::new("x")).unwrap();

    let diff = store.diff_chain("", &c2).unwrap();
    assert!(diff.iter().all(|e| e.op == "create"),
        "all ops should be 'create' when from is empty: {:?}", diff);
    assert_eq!(diff.len(), 2);
}

// ── P0.2 content-aware conflict detection ─────────────────────────────────

#[test]
fn same_content_both_stacks_no_conflict() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    // Both write the same content to the same file
    store.edit(&sa, "src/shared.ts", b"same content", Intent::new("a")).unwrap();
    store.edit(&sb, "src/shared.ts", b"same content", Intent::new("b")).unwrap();

    let view_id = store.open_view("".to_owned(), &[sa, sb]).unwrap();
    let conflicts = store.conflicts(&view_id).unwrap();
    assert!(conflicts.is_empty(), "same content should not conflict: {:?}", conflicts);
}

#[test]
fn different_content_both_stacks_is_conflict() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    store.edit(&sa, "src/calc.ts", b"version A", Intent::new("a")).unwrap();
    store.edit(&sb, "src/calc.ts", b"version B", Intent::new("b")).unwrap();

    let view_id = store.open_view("".to_owned(), &[sa, sb]).unwrap();
    let conflicts = store.conflicts(&view_id).unwrap();
    assert!(!conflicts.is_empty(), "different content should produce a conflict");
    assert_eq!(conflicts[0].path, "src/calc.ts");
}

#[test]
fn only_one_stack_modified_no_conflict() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    // Only stack A touches the file
    store.edit(&sa, "src/only-a.ts", b"A's version", Intent::new("a")).unwrap();
    // Stack B never touches it

    let view_id = store.open_view("".to_owned(), &[sa, sb]).unwrap();
    let conflicts = store.conflicts(&view_id).unwrap();
    assert!(conflicts.is_empty(), "one-stack-only should not conflict");

    // A's content should appear in the view
    let files = store.list_files(&view_id).unwrap();
    assert!(files.contains(&"src/only-a.ts".to_owned()));
}

#[test]
fn overview_will_conflict_false_for_same_content() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    store.edit(&sa, "src/x.ts", b"identical", Intent::new("a")).unwrap();
    store.edit(&sb, "src/x.ts", b"identical", Intent::new("b")).unwrap();

    let ov = store.overview().unwrap();
    // The file appears as hot (touched by 2 stacks) but will_conflict=false
    let hot = ov.hot_files.iter().find(|h| h.path == "src/x.ts");
    assert!(hot.is_some(), "file should appear in hot_files");
    assert!(!hot.unwrap().will_conflict, "will_conflict should be false for same content");
}

#[test]
fn overview_will_conflict_true_for_different_content() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    store.edit(&sa, "src/y.ts", b"version A", Intent::new("a")).unwrap();
    store.edit(&sb, "src/y.ts", b"version B", Intent::new("b")).unwrap();

    let ov = store.overview().unwrap();
    let hot = ov.hot_files.iter().find(|h| h.path == "src/y.ts");
    assert!(hot.is_some());
    assert!(hot.unwrap().will_conflict, "will_conflict should be true for different content");
}

// ── P1.3 blob GC ───────────────────────────────────────────────────────────

#[test]
fn gc_returns_zero_when_all_referenced() {
    let (store, _dir) = store();
    let stack = store.open_stack("agent", None).unwrap();
    store.edit(&stack, "src/a.ts", b"hello", Intent::new("x")).unwrap();

    let freed = store.gc().unwrap();
    assert_eq!(freed, 0, "no blobs should be freed when stack is open");
}

#[test]
fn gc_frees_blobs_from_abandoned_stack() {
    let (store, _dir) = store();
    let stack = store.open_stack("agent", None).unwrap();
    store.edit(&stack, "src/orphan.ts", b"orphaned content xyzabc123", Intent::new("x")).unwrap();
    store.abandon_stack(&stack).unwrap();

    let freed = store.gc().unwrap();
    assert!(freed > 0, "blobs from abandoned stack should be freed");
}

#[test]
fn gc_idempotent() {
    let (store, _dir) = store();
    let stack = store.open_stack("agent", None).unwrap();
    store.edit(&stack, "src/a.ts", b"live", Intent::new("x")).unwrap();
    store.abandon_stack(&stack).unwrap();

    let freed1 = store.gc().unwrap();
    let freed2 = store.gc().unwrap();
    assert!(freed1 > 0);
    assert_eq!(freed2, 0, "second GC should find nothing to free");
}

#[test]
fn gc_keeps_blobs_from_live_stack() {
    let (store, _dir) = store();
    let live_stack = store.open_stack("live", None).unwrap();
    let dead_stack = store.open_stack("dead", None).unwrap();

    store.edit(&live_stack, "src/live.ts", b"keep this", Intent::new("x")).unwrap();
    store.edit(&dead_stack, "src/dead.ts", b"remove this", Intent::new("x")).unwrap();
    store.abandon_stack(&dead_stack).unwrap();

    let freed = store.gc().unwrap();
    // At least one blob freed (from dead stack)
    assert!(freed >= 1);
    // Live stack's file still readable
    let tip = store.list_stacks().unwrap()
        .into_iter()
        .find(|s| s.stack_id == live_stack)
        .unwrap()
        .tip_change_id
        .unwrap();
    let snap = store.snapshot_at(&tip).unwrap();
    assert!(snap.contains_key("src/live.ts"), "live blob must not be GC'd");
}

// ── P2: content-aware file_contention ─────────────────────────────────────

#[test]
fn contention_same_content_is_not_reported() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    // Both write identical content — should NOT appear in contention
    store.edit(&sa, "src/shared.ts", b"same content", Intent::new("a")).unwrap();
    store.edit(&sb, "src/shared.ts", b"same content", Intent::new("b")).unwrap();

    let ct = store.file_contention("src/shared.ts", &sa).unwrap();
    assert!(ct.other_stacks.is_empty(),
        "same-content writes must not produce contention: {:?}", ct.other_stacks);
}

#[test]
fn contention_different_content_is_reported() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    store.edit(&sa, "src/calc.ts", b"version A", Intent::new("a")).unwrap();
    store.edit(&sb, "src/calc.ts", b"version B", Intent::new("b")).unwrap();

    let ct = store.file_contention("src/calc.ts", &sa).unwrap();
    assert_eq!(ct.other_stacks.len(), 1, "different content must appear in contention");
    assert_eq!(ct.other_stacks[0].agent_id, "agent-b");
    // blob_hash must be populated so callers can compare without extra lookups
    assert!(ct.other_stacks[0].blob_hash.is_some());
}

#[test]
fn contention_only_other_stack_not_caller() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();

    store.edit(&sa, "src/a.ts", b"content", Intent::new("a")).unwrap();

    // Caller is excluded — should never see itself
    let ct = store.file_contention("src/a.ts", &sa).unwrap();
    assert!(ct.other_stacks.is_empty(), "caller's own stack must not appear in contention");
}

#[test]
fn contention_closed_stack_not_reported() {
    let (store, _dir) = store();
    let sa = store.open_stack("agent-a", None).unwrap();
    let sb = store.open_stack("agent-b", None).unwrap();

    store.edit(&sa, "src/x.ts", b"version A", Intent::new("a")).unwrap();
    store.edit(&sb, "src/x.ts", b"version B", Intent::new("b")).unwrap();
    // Close stack B — done with its work
    store.close_stack(&sb).unwrap();

    // A is still open; B is closed → no live contention
    let ct = store.file_contention("src/x.ts", &sa).unwrap();
    assert!(ct.other_stacks.is_empty(),
        "closed stack must not appear in live contention: {:?}", ct.other_stacks);
}
