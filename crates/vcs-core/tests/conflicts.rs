//! M4 — conflicts as data.

use tempfile::TempDir;
use vcs_core::{Intent, Resolution, Store};

fn store() -> (Store, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Store::init(dir.path()).unwrap(), dir)
}

#[test]
fn same_file_two_stacks_produces_conflict() {
    let (store, _dir) = store();

    // Common base
    let seed = store.open_stack("seed", None).unwrap();
    let base = store
        .edit(&seed, "config.toml", b"[base]", Intent::new("initial config"))
        .unwrap();
    store.close_stack(&seed).unwrap();

    // Agent A edits config.toml
    let stack_a = store.open_stack("agent-a", Some(base.clone())).unwrap();
    store
        .edit(&stack_a, "config.toml", b"[base]\nfoo=1", Intent::new("add foo"))
        .unwrap();
    store.close_stack(&stack_a).unwrap();

    // Agent B also edits config.toml (conflict!)
    let stack_b = store.open_stack("agent-b", Some(base.clone())).unwrap();
    store
        .edit(&stack_b, "config.toml", b"[base]\nbar=2", Intent::new("add bar"))
        .unwrap();
    store.close_stack(&stack_b).unwrap();

    // Open view — conflicts should be detected
    let view = store
        .open_view(base.clone(), &[stack_a.clone(), stack_b.clone()])
        .unwrap();

    let conflicts = store.conflicts(&view).unwrap();
    assert_eq!(conflicts.len(), 1, "expected exactly one conflict");

    let c = &conflicts[0];
    assert_eq!(c.path, "config.toml");
    assert_eq!(c.candidates.len(), 2);
    assert!(c.resolution.is_none(), "conflict should start unresolved");

    // Reading the conflicted file should fail
    let err = store.read_file(&view, "config.toml").unwrap_err();
    assert!(err.to_string().contains("conflict"), "expected conflict error, got: {err}");
}

#[test]
fn resolve_by_pick_makes_file_readable() {
    let (store, _dir) = store();

    let seed = store.open_stack("seed", None).unwrap();
    let base = store
        .edit(&seed, "main.py", b"print('base')", Intent::new("base"))
        .unwrap();
    store.close_stack(&seed).unwrap();

    let sa = store.open_stack("a", Some(base.clone())).unwrap();
    store
        .edit(&sa, "main.py", b"print('A')", Intent::new("A version"))
        .unwrap();
    store.close_stack(&sa).unwrap();

    let sb = store.open_stack("b", Some(base.clone())).unwrap();
    store
        .edit(&sb, "main.py", b"print('B')", Intent::new("B version"))
        .unwrap();
    store.close_stack(&sb).unwrap();

    let view = store
        .open_view(base.clone(), &[sa.clone(), sb.clone()])
        .unwrap();

    let conflicts = store.conflicts(&view).unwrap();
    assert_eq!(conflicts.len(), 1);

    // Pick stack A as winner
    store
        .resolve(&conflicts[0].conflict_id, Resolution::Pick { stack_id: sa.clone() })
        .unwrap();

    let content = store.read_file(&view, "main.py").unwrap();
    assert_eq!(content, b"print('A')");
}

#[test]
fn resolve_by_merge_blob_makes_file_readable() {
    let (store, _dir) = store();

    let seed = store.open_stack("seed", None).unwrap();
    let base = store
        .edit(&seed, "data.json", b"{}", Intent::new("empty"))
        .unwrap();
    store.close_stack(&seed).unwrap();

    let sa = store.open_stack("a", Some(base.clone())).unwrap();
    store.edit(&sa, "data.json", b"{\"a\":1}", Intent::new("a")).unwrap();
    store.close_stack(&sa).unwrap();

    let sb = store.open_stack("b", Some(base.clone())).unwrap();
    store.edit(&sb, "data.json", b"{\"b\":2}", Intent::new("b")).unwrap();
    store.close_stack(&sb).unwrap();

    let view = store.open_view(base.clone(), &[sa, sb]).unwrap();
    let conflicts = store.conflicts(&view).unwrap();

    // The merged blob is computed externally (orchestrator's job)
    let merged_blob = b"{\"a\":1,\"b\":2}";
    let merged_hash = store.put_blob(merged_blob).unwrap();

    store
        .resolve(
            &conflicts[0].conflict_id,
            Resolution::Merge { blob_hash: merged_hash },
        )
        .unwrap();

    let content = store.read_file(&view, "data.json").unwrap();
    assert_eq!(content, b"{\"a\":1,\"b\":2}");
}

#[test]
fn non_conflicting_files_readable_despite_conflict_on_other_path() {
    let (store, _dir) = store();

    let seed = store.open_stack("seed", None).unwrap();
    let base = store
        .edit(&seed, "ok.txt", b"fine", Intent::new("ok"))
        .unwrap();
    store.close_stack(&seed).unwrap();

    let sa = store.open_stack("a", Some(base.clone())).unwrap();
    store.edit(&sa, "conflict.txt", b"version A", Intent::new("a")).unwrap();
    store.edit(&sa, "only_a.txt", b"only in A", Intent::new("only a")).unwrap();
    store.close_stack(&sa).unwrap();

    let sb = store.open_stack("b", Some(base.clone())).unwrap();
    store.edit(&sb, "conflict.txt", b"version B", Intent::new("b")).unwrap();
    store.close_stack(&sb).unwrap();

    let view = store.open_view(base.clone(), &[sa, sb]).unwrap();

    // ok.txt and only_a.txt are readable despite conflict on conflict.txt
    let ok = store.read_file(&view, "ok.txt").unwrap();
    assert_eq!(ok, b"fine");

    let only_a = store.read_file(&view, "only_a.txt").unwrap();
    assert_eq!(only_a, b"only in A");

    // conflict.txt is NOT readable
    assert!(store.read_file(&view, "conflict.txt").is_err());
}
