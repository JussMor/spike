//! Store — the single entry-point for all VCS operations.
//!
//! Layout on disk:
//!   <root>/vcs.db          SQLite database
//!   <root>/blobs/          content-addressed blob store

use crate::blob::BlobStore;
use crate::change::{
    compute_change_id, Change, ChangeId, ConflictId, EditKind, EditMetadata, EditPatch, Op,
    StackId, ViewId,
};
use crate::error::{Result, VcsError};
use crate::hub::{HubBundle, HubChange, HubEditMetadata, HubFileEntry, HubStack};
use crate::intent::Intent;
use crate::session::{
    AgentOverview, ContentionEntry, FileContention, Session, SessionSummary,
};
use crate::stack::{Stack, StackStatus};
use crate::view::{state_hash, Candidate, Conflict, Resolution, View};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = include_str!("schema.sql");

pub struct Store {
    conn: Connection,
    blobs: BlobStore,
    root: PathBuf,
}

// ── helpers ────────────────────────────────────────────────────────────────

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ── Store lifecycle ────────────────────────────────────────────────────────

impl Store {
    /// Create a new store at `path`, fail if already initialised.
    pub fn init(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        let db_path = path.join("vcs.db");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(SCHEMA)?;
        let blobs = BlobStore::new(path)?;
        Ok(Self {
            conn,
            blobs,
            root: path.to_path_buf(),
        })
    }

    /// Open an existing store at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let db_path = path.join("vcs.db");
        if !db_path.exists() {
            return Err(VcsError::NotInitialised(path.display().to_string()));
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        let blobs = BlobStore::new(path)?;
        Ok(Self {
            conn,
            blobs,
            root: path.to_path_buf(),
        })
    }

    /// Open if exists, init if not.
    pub fn open_or_init(path: &Path) -> Result<Self> {
        if path.join("vcs.db").exists() {
            Self::open(path)
        } else {
            Self::init(path)
        }
    }
}

// ── Stack lifecycle ────────────────────────────────────────────────────────

impl Store {
    /// Start a new stack for `agent_id`, optionally branching from `base`.
    pub fn open_stack(&self, agent_id: &str, base: Option<ChangeId>) -> Result<StackId> {
        let stack_id = new_id();
        self.conn.execute(
            "INSERT INTO stacks (stack_id, agent_id, base_change_id, tip_change_id, status)
             VALUES (?1, ?2, ?3, ?3, 'open')",
            params![stack_id, agent_id, base],
        )?;
        tracing::debug!(%stack_id, %agent_id, "stack opened");
        Ok(stack_id)
    }

    /// Mark a stack closed (work is done).
    pub fn close_stack(&self, stack: &StackId) -> Result<()> {
        self.set_stack_status(stack, StackStatus::Closed)
    }

    /// Mark a stack abandoned.
    pub fn abandon_stack(&self, stack: &StackId) -> Result<()> {
        self.set_stack_status(stack, StackStatus::Abandoned)
    }

    fn set_stack_status(&self, stack: &StackId, status: StackStatus) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE stacks SET status=?1 WHERE stack_id=?2",
            params![status.to_string(), stack],
        )?;
        if n == 0 {
            return Err(VcsError::StackNotFound(stack.clone()));
        }
        Ok(())
    }

    /// Load a stack record.
    pub fn get_stack(&self, stack_id: &str) -> Result<Stack> {
        self.conn
            .query_row(
                "SELECT stack_id, agent_id, base_change_id, tip_change_id, status
                 FROM stacks WHERE stack_id=?1",
                params![stack_id],
                |row| {
                    Ok(Stack {
                        stack_id: row.get(0)?,
                        agent_id: row.get(1)?,
                        base_change_id: row.get(2)?,
                        tip_change_id: row.get(3)?,
                        status: {
                            let s: String = row.get(4)?;
                            s.parse::<StackStatus>().unwrap_or(StackStatus::Open)
                        },
                    })
                },
            )
            .optional()?
            .ok_or_else(|| VcsError::StackNotFound(stack_id.to_owned()))
    }
}

// ── Edits ──────────────────────────────────────────────────────────────────

impl Store {
    /// Record a create-or-edit of `path` with `new_content`.
    pub fn edit(
        &self,
        stack: &StackId,
        path: &str,
        new_content: &[u8],
        intent: Intent,
    ) -> Result<ChangeId> {
        let stk = self.require_open_stack(stack)?;

        let base_blob_hash = self.current_file_blob_for_stack(&stk, path)?;
        let base_content = base_blob_hash
            .as_deref()
            .map(|hash| self.blobs.get(hash))
            .transpose()?;
        let edit_plan =
            build_edit_patch(base_blob_hash.clone(), base_content.as_deref(), new_content);

        let result_blob_hash = self.blobs.put(new_content)?;
        let patch_blob_hash = self.blobs.put(&serde_json::to_vec(&edit_plan.patch)?)?;

        let op = if base_blob_hash.is_some() {
            Op::Edit
        } else {
            Op::Create
        };

        let parent_id = stk.tip_change_id.as_deref();
        let ts = now_ms();
        let change_id =
            compute_change_id(parent_id, path, Some(&patch_blob_hash), &stk.agent_id, ts);

        self.insert_change(
            &change_id,
            parent_id,
            path,
            &op,
            Some(&patch_blob_hash),
            &stk.agent_id,
            &intent,
            ts,
        )?;
        self.upsert_files_at_change(&change_id, path, Some(&result_blob_hash))?;
        self.insert_edit_metadata(&EditMetadata {
            change_id: change_id.clone(),
            path: path.to_owned(),
            base_blob_hash,
            result_blob_hash: Some(result_blob_hash),
            patch_blob_hash: Some(patch_blob_hash),
            edit_kind: edit_plan.edit_kind,
            start_line: edit_plan.start_line,
            end_line: edit_plan.end_line,
            inserted_lines: edit_plan.inserted_lines,
            deleted_lines: edit_plan.deleted_lines,
        })?;
        self.advance_stack_tip(stack, &change_id)?;

        tracing::debug!(%change_id, %path, op=%op, "edit recorded");
        Ok(change_id)
    }

    /// Record deletion of `path`.
    pub fn delete(&self, stack: &StackId, path: &str, intent: Intent) -> Result<ChangeId> {
        let stk = self.require_open_stack(stack)?;
        let base_blob_hash = self.current_file_blob_for_stack(&stk, path)?;
        let patch = EditPatch::Delete {
            base_blob_hash: base_blob_hash.clone(),
        };
        let patch_blob_hash = self.blobs.put(&serde_json::to_vec(&patch)?)?;
        let parent_id = stk.tip_change_id.as_deref();
        let ts = now_ms();
        let change_id =
            compute_change_id(parent_id, path, Some(&patch_blob_hash), &stk.agent_id, ts);

        self.insert_change(
            &change_id,
            parent_id,
            path,
            &Op::Delete,
            Some(&patch_blob_hash),
            &stk.agent_id,
            &intent,
            ts,
        )?;
        self.upsert_files_at_change(&change_id, path, None)?;
        self.insert_edit_metadata(&EditMetadata {
            change_id: change_id.clone(),
            path: path.to_owned(),
            base_blob_hash,
            result_blob_hash: None,
            patch_blob_hash: Some(patch_blob_hash),
            edit_kind: EditKind::Delete,
            start_line: None,
            end_line: None,
            inserted_lines: 0,
            deleted_lines: 0,
        })?;
        self.advance_stack_tip(stack, &change_id)?;

        tracing::debug!(%change_id, %path, "delete recorded");
        Ok(change_id)
    }

    /// Record renaming `from` → `to` with `new_content` (the file at its new location).
    pub fn rename(
        &self,
        stack: &StackId,
        from: &str,
        to: &str,
        new_content: &[u8],
        intent: Intent,
    ) -> Result<ChangeId> {
        let stk = self.require_open_stack(stack)?;
        let base_blob_hash = self.current_file_blob_for_stack(&stk, from)?;
        let blob_hash = self.blobs.put(new_content)?;
        let patch = EditPatch::Rename {
            from: from.to_owned(),
            to: to.to_owned(),
            base_blob_hash: base_blob_hash.clone(),
            result_blob_hash: blob_hash.clone(),
        };
        let patch_blob_hash = self.blobs.put(&serde_json::to_vec(&patch)?)?;
        let path = format!("{from}\x00{to}"); // encode both paths in the path field
        let parent_id = stk.tip_change_id.as_deref();
        let ts = now_ms();
        let change_id =
            compute_change_id(parent_id, &path, Some(&patch_blob_hash), &stk.agent_id, ts);

        self.insert_change(
            &change_id,
            parent_id,
            &path,
            &Op::Rename,
            Some(&patch_blob_hash),
            &stk.agent_id,
            &intent,
            ts,
        )?;
        // Delete old path, create new path in derived index
        self.upsert_files_at_change(&change_id, from, None)?;
        self.upsert_files_at_change(&change_id, to, Some(&blob_hash))?;
        self.insert_edit_metadata(&EditMetadata {
            change_id: change_id.clone(),
            path: to.to_owned(),
            base_blob_hash,
            result_blob_hash: Some(blob_hash),
            patch_blob_hash: Some(patch_blob_hash),
            edit_kind: EditKind::Rename,
            start_line: None,
            end_line: None,
            inserted_lines: 0,
            deleted_lines: 0,
        })?;
        self.advance_stack_tip(stack, &change_id)?;

        tracing::debug!(%change_id, %from, %to, "rename recorded");
        Ok(change_id)
    }

    // ── internal write helpers ─────────────────────────────────────────────

    fn insert_change(
        &self,
        change_id: &str,
        parent_id: Option<&str>,
        path: &str,
        op: &Op,
        diff_hash: Option<&str>,
        agent_id: &str,
        intent: &Intent,
        ts: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO changes
             (change_id, parent_id, path, op, diff_hash, agent_id, intent, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                change_id,
                parent_id,
                path,
                op.to_string(),
                diff_hash,
                agent_id,
                intent.to_json()?,
                ts,
            ],
        )?;
        Ok(())
    }

    fn upsert_files_at_change(
        &self,
        change_id: &str,
        path: &str,
        blob_hash: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO files_at_change (change_id, path, blob_hash)
             VALUES (?1, ?2, ?3)",
            params![change_id, path, blob_hash],
        )?;
        Ok(())
    }

    fn insert_edit_metadata(&self, meta: &EditMetadata) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO edit_metadata
             (change_id, path, base_blob_hash, result_blob_hash, patch_blob_hash,
              edit_kind, start_line, end_line, inserted_lines, deleted_lines)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                meta.change_id,
                meta.path,
                meta.base_blob_hash,
                meta.result_blob_hash,
                meta.patch_blob_hash,
                meta.edit_kind.to_string(),
                meta.start_line,
                meta.end_line,
                meta.inserted_lines,
                meta.deleted_lines,
            ],
        )?;
        Ok(())
    }

    fn advance_stack_tip(&self, stack_id: &str, change_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE stacks SET tip_change_id=?1 WHERE stack_id=?2",
            params![change_id, stack_id],
        )?;
        Ok(())
    }

    fn require_open_stack(&self, stack_id: &str) -> Result<Stack> {
        let s = self.get_stack(stack_id)?;
        if s.status != StackStatus::Open {
            return Err(VcsError::StackNotOpen(
                stack_id.to_owned(),
                s.status.to_string(),
            ));
        }
        Ok(s)
    }

    fn current_file_blob_for_stack(&self, stk: &Stack, path: &str) -> Result<Option<String>> {
        let stack_snapshot = self.stack_snapshot(&stk.stack_id)?;
        if let Some(blob) = stack_snapshot.get(path) {
            return Ok(blob.clone());
        }
        let Some(base) = &stk.base_change_id else {
            return Ok(None);
        };
        let base_snapshot = self.snapshot_at(base)?;
        Ok(base_snapshot.get(path).cloned())
    }
}

// ── Reads ──────────────────────────────────────────────────────────────────

impl Store {
    /// Open a view over `base` with `stacks` applied.
    pub fn open_view(&self, base: ChangeId, stacks: &[StackId]) -> Result<ViewId> {
        let view_id = new_id();

        // Build per-stack file snapshots
        let stack_snaps = self.build_stack_snapshots(stacks)?;

        // Build base snapshot
        let base_snap = self.snapshot_at(&base)?;

        // Compute merged tree with enhanced candidate tracking
        let merged = self.compute_merge_with_candidates(&base_snap, &stack_snaps)?;

        // State hash over clean paths
        let sh = state_hash(&merged.clean);

        // Store view
        let stacks_json = serde_json::to_string(stacks)?;
        self.conn.execute(
            "INSERT INTO views (view_id, base_change_id, applied_stacks, state_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![view_id, base, stacks_json, sh],
        )?;

        // Persist conflicts
        for (path, candidates) in &merged.conflicts {
            let conflict_id = new_id();
            let candidates_json = serde_json::to_string(candidates)?;
            self.conn.execute(
                "INSERT INTO conflicts (conflict_id, view_id, path, candidates, resolution)
                 VALUES (?1, ?2, ?3, ?4, NULL)",
                params![conflict_id, view_id, path, candidates_json],
            )?;
        }
        for (path, (candidates, resolution)) in &merged.auto_resolved {
            let conflict_id = new_id();
            let candidates_json = serde_json::to_string(candidates)?;
            let resolution_json = serde_json::to_string(resolution)?;
            self.conn.execute(
                "INSERT INTO conflicts (conflict_id, view_id, path, candidates, resolution)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![conflict_id, view_id, path, candidates_json, resolution_json],
            )?;
        }

        tracing::debug!(%view_id, base=%base, stacks=?stacks, conflicts=%merged.conflicts.len(), "view opened");
        Ok(view_id)
    }

    /// Read a file's content through a view.
    pub fn read_file(&self, view: &ViewId, path: &str) -> Result<Vec<u8>> {
        let v = self.get_view(view)?;

        // Check for unresolved conflicts on this path
        let unresolved: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM conflicts
                 WHERE view_id=?1 AND path=?2 AND resolution IS NULL",
                params![view, path],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);

        if unresolved {
            return Err(VcsError::UnresolvedConflicts(view.clone()));
        }

        // Check for a resolved conflict — use its resolution blob
        let resolution: Option<String> = self
            .conn
            .query_row(
                "SELECT resolution FROM conflicts
                 WHERE view_id=?1 AND path=?2 AND resolution IS NOT NULL",
                params![view, path],
                |r| r.get(0),
            )
            .optional()?
            .flatten();

        if let Some(res_json) = resolution {
            let res: Resolution = serde_json::from_str(&res_json)?;
            return match res {
                Resolution::Pick { stack_id } => {
                    // Find that stack's version
                    let blob = self.stack_file_blob(&stack_id, path)?;
                    blob.map(|h| self.blobs.get(&h))
                        .transpose()?
                        .ok_or_else(|| VcsError::FileNotFound(path.to_owned()))
                }
                Resolution::Merge { blob_hash } => self.blobs.get(&blob_hash),
            };
        }

        // No conflict — find the file in the merged view
        let stacks: Vec<StackId> = serde_json::from_str(&v.applied_stacks)?;

        // Check stacks newest-first
        for stack_id in stacks.iter().rev() {
            if let Some(hash) = self.stack_file_blob(stack_id, path)? {
                return self.blobs.get(&hash);
            }
        }

        // Fall back to base
        let base_blob: Option<String> = self
            .conn
            .query_row(
                "SELECT blob_hash FROM files_at_change WHERE change_id=?1 AND path=?2",
                params![v.base_change_id, path],
                |r| r.get(0),
            )
            .optional()?
            .flatten();

        match base_blob {
            Some(h) => self.blobs.get(&h),
            None => Err(VcsError::FileNotFound(path.to_owned())),
        }
    }

    /// List all files visible through a view (clean + conflict paths).
    pub fn list_files(&self, view: &ViewId) -> Result<Vec<String>> {
        let v = self.get_view(view)?;
        let stacks: Vec<StackId> = serde_json::from_str(&v.applied_stacks)?;

        let mut paths = std::collections::BTreeSet::new();

        // Files at base
        let base_paths = self.files_at_change_id(&v.base_change_id)?;
        paths.extend(base_paths);

        // Files touched by stacks
        for sid in &stacks {
            let snap = self.stack_snapshot(sid)?;
            for (path, blob) in snap {
                if blob.is_some() {
                    paths.insert(path);
                } else {
                    paths.remove(&path); // deleted
                }
            }
        }

        Ok(paths.into_iter().collect())
    }

    // ── conflict API ───────────────────────────────────────────────────────

    /// Return all conflicts in a view (resolved or not).
    pub fn conflicts(&self, view: &ViewId) -> Result<Vec<Conflict>> {
        let mut stmt = self.conn.prepare(
            "SELECT conflict_id, view_id, path, candidates, resolution
             FROM conflicts WHERE view_id=?1",
        )?;
        let rows = stmt.query_map(params![view], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (conflict_id, view_id, path, candidates_json, resolution_json) = row?;
            let candidates: Vec<Candidate> = serde_json::from_str(&candidates_json)?;
            let resolution: Option<Resolution> = resolution_json
                .map(|s| serde_json::from_str(&s))
                .transpose()?;
            out.push(Conflict {
                conflict_id,
                view_id,
                path,
                candidates,
                resolution,
            });
        }
        Ok(out)
    }

    /// Resolve a conflict.
    pub fn resolve(&self, conflict_id: &ConflictId, resolution: Resolution) -> Result<()> {
        let res_json = serde_json::to_string(&resolution)?;
        let n = self.conn.execute(
            "UPDATE conflicts SET resolution=?1 WHERE conflict_id=?2",
            params![res_json, conflict_id],
        )?;
        if n == 0 {
            return Err(VcsError::ConflictNotFound(conflict_id.clone()));
        }
        tracing::debug!(%conflict_id, "conflict resolved");
        Ok(())
    }

    // ── Inspection ─────────────────────────────────────────────────────────

    /// Return the change log for a stack, oldest-first.
    pub fn log(&self, stack: &StackId) -> Result<Vec<Change>> {
        let stk = self.get_stack(stack)?;
        let Some(tip) = stk.tip_change_id else {
            return Ok(vec![]);
        };

        // Walk the parent chain from tip back to base
        let mut chain: Vec<Change> = Vec::new();
        let mut current = Some(tip);

        while let Some(cid) = current {
            // Stop at the base (don't include changes from previous stacks)
            if stk.base_change_id.as_deref() == Some(&cid) {
                break;
            }

            let change = self.get_change(&cid)?;
            current = change.parent_id.clone();
            chain.push(change);
        }

        chain.reverse(); // oldest first
        Ok(chain)
    }

    /// Return a simple diff summary between two change IDs (snapshot diff).
    pub fn diff(&self, from: &ChangeId, to: &ChangeId) -> Result<Vec<DiffEntry>> {
        let from_snap = self.snapshot_at(from)?;
        let to_snap = self.snapshot_at(to)?;

        let mut entries = Vec::new();
        let mut all_paths: std::collections::BTreeSet<&str> =
            from_snap.keys().map(|s| s.as_str()).collect();
        all_paths.extend(to_snap.keys().map(|s| s.as_str()));

        for path in all_paths {
            let before = from_snap.get(path).cloned();
            let after = to_snap.get(path).cloned();
            if before != after {
                entries.push(DiffEntry {
                    path: path.to_owned(),
                    before_hash: before,
                    after_hash: after,
                });
            }
        }
        Ok(entries)
    }

    /// Walk the change chain from `to` back to `from`, returning all paths
    /// touched in between (most-recent-wins per path).
    ///
    /// This is what the vcs-vite HMR poller needs: given `prevTip` and `newTip`,
    /// find exactly which files changed so only those modules are invalidated.
    ///
    /// The op is derived by comparing each path against the snapshot AT `from`:
    ///   - not-present → present hash: "create"
    ///   - present hash → different hash: "edit"
    ///   - present hash → not-present: "delete"
    pub fn diff_chain(&self, from: &str, to: &str) -> Result<Vec<ChainDiffEntry>> {
        if from == to || to.is_empty() {
            return Ok(vec![]);
        }

        // Baseline snapshot at `from` (may be empty string = root)
        let base_snap = if from.is_empty() {
            HashMap::new()
        } else {
            self.snapshot_at(from)?
        };

        // Walk tip→from collecting first-seen (newest) state per path
        let mut path_ops: HashMap<String, Option<String>> = HashMap::new();
        let mut current = Some(to.to_owned());

        while let Some(cid) = current {
            if cid == from {
                break;
            }
            let change = self.get_change(&cid)?;
            for entry in self.file_entries_for_change(&cid)? {
                path_ops.entry(entry.path).or_insert(entry.blob_hash);
            }
            current = change.parent_id;
        }

        let mut entries: Vec<ChainDiffEntry> = path_ops
            .into_iter()
            .map(|(path, after_blob)| {
                let before = base_snap.get(&path).cloned();
                let op = match (&before, &after_blob) {
                    (None, Some(_)) => "create",
                    (Some(_), None) => "delete",
                    (Some(b), Some(a)) if b != a => "edit",
                    _ => "noop",
                };
                ChainDiffEntry {
                    path,
                    op: op.to_owned(),
                    blob_hash: after_blob,
                }
            })
            .filter(|e| e.op != "noop")
            .collect();

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// Garbage-collect unreferenced blobs.
    ///
    /// Walks every non-abandoned stack's full change chain, collects all
    /// referenced blob hashes, then removes any blob file not in that set.
    /// Returns the number of blobs freed.
    pub fn gc(&self) -> Result<usize> {
        let mut live: BTreeSet<String> = BTreeSet::new();

        // Collect live blobs from all non-abandoned stacks
        let stacks = self.list_stacks()?;
        for stk in &stacks {
            if stk.status == StackStatus::Abandoned {
                continue;
            }
            let Some(ref tip) = stk.tip_change_id else { continue; };
            let mut current = Some(tip.clone());
            while let Some(cid) = current {
                if stk.base_change_id.as_deref() == Some(cid.as_str()) {
                    break;
                }
                let change = match self.get_change(&cid) {
                    Ok(c) => c,
                    Err(_) => break,
                };
                if let Some(h) = &change.diff_hash {
                    live.insert(h.clone());
                }
                for entry in self.file_entries_for_change(&cid).unwrap_or_default() {
                    if let Some(h) = entry.blob_hash {
                        live.insert(h);
                    }
                }
                if let Ok(Some(meta)) = self.get_edit_metadata(&cid) {
                    if let Some(h) = meta.base_blob_hash   { live.insert(h); }
                    if let Some(h) = meta.result_blob_hash { live.insert(h); }
                    if let Some(h) = meta.patch_blob_hash  { live.insert(h); }
                }
                current = change.parent_id;
            }
        }

        // Collect blobs referenced by conflict candidates and resolutions
        let mut stmt = self.conn.prepare(
            "SELECT candidates, resolution FROM conflicts",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        })?;
        for row in rows {
            let (candidates_json, resolution_json) = row?;
            if let Ok(candidates) = serde_json::from_str::<Vec<Candidate>>(&candidates_json) {
                for c in candidates {
                    if let Some(h) = c.blob_hash { live.insert(h); }
                }
            }
            if let Some(res_json) = resolution_json {
                if let Ok(Resolution::Merge { blob_hash }) = serde_json::from_str(&res_json) {
                    live.insert(blob_hash);
                }
            }
        }

        // Walk blob directory and remove unreferenced files
        let blobs_root = self.root.join("blobs");
        if !blobs_root.exists() {
            return Ok(0);
        }
        let mut freed = 0usize;

        for prefix_entry in std::fs::read_dir(&blobs_root)? {
            let prefix_entry = prefix_entry?;
            if !prefix_entry.file_type()?.is_dir() { continue; }
            let prefix = prefix_entry.file_name().to_string_lossy().to_string();

            for blob_entry in std::fs::read_dir(prefix_entry.path())? {
                let blob_entry = blob_entry?;
                let filename = blob_entry.file_name().to_string_lossy().to_string();
                if filename.ends_with(".tmp") { continue; }

                let full_hash = format!("{prefix}{filename}");
                if !live.contains(&full_hash) {
                    // Ignore NotFound errors (concurrent GC or already gone)
                    let _ = std::fs::remove_file(blob_entry.path());
                    freed += 1;
                }
            }
        }

        Ok(freed)
    }

    // ── Blob passthrough ───────────────────────────────────────────────────

    /// Store raw bytes, return BLAKE3 hash. Useful for pre-seeding blobs.
    pub fn put_blob(&self, data: &[u8]) -> Result<String> {
        self.blobs.put(data)
    }

    /// Fetch raw bytes by hash.
    pub fn get_blob(&self, hash: &str) -> Result<Vec<u8>> {
        self.blobs.get(hash)
    }

    /// All paths ever tracked as present in this store.
    pub fn list_tracked_paths(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT path FROM files_at_change
             WHERE blob_hash IS NOT NULL
             ORDER BY path",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// The composed file tree state at a given change_id.
    pub fn snapshot_at(&self, change_id: &str) -> Result<HashMap<String, String>> {
        if change_id.is_empty() {
            return Ok(HashMap::new());
        }

        let mut state: HashMap<String, Option<String>> = HashMap::new();
        let mut current = Some(change_id.to_owned());

        while let Some(cid) = current {
            let change = self.get_change(&cid)?;
            let entries = self.file_entries_for_change(&cid)?;
            for entry in entries {
                state.entry(entry.path).or_insert(entry.blob_hash);
            }
            current = change.parent_id;
        }

        Ok(state
            .into_iter()
            .filter_map(|(path, blob)| blob.map(|hash| (path, hash)))
            .collect())
    }

    // ── Listing helpers (used by serve + remote clients) ───────────────────

    /// All stacks, newest first.
    pub fn list_stacks(&self) -> Result<Vec<Stack>> {
        let mut stmt = self.conn.prepare(
            "SELECT stack_id, agent_id, base_change_id, tip_change_id, status
             FROM stacks ORDER BY rowid DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (sid, aid, base, tip, status_s) = row?;
            out.push(Stack {
                stack_id: sid,
                agent_id: aid,
                base_change_id: base,
                tip_change_id: tip,
                status: status_s.parse().unwrap_or(StackStatus::Open),
            });
        }
        Ok(out)
    }

    /// All changes, newest first.
    pub fn list_changes(&self) -> Result<Vec<Change>> {
        let mut stmt = self.conn.prepare(
            "SELECT change_id, parent_id, path, op, diff_hash, agent_id, intent, created_at
             FROM changes ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (cid, pid, path, op_s, dh, aid, intent_s, ts) = row?;
            out.push(Change {
                change_id: cid,
                parent_id: pid,
                path,
                op: op_s.parse()?,
                diff_hash: dh,
                agent_id: aid,
                intent: Intent::from_json(&intent_s)?,
                created_at: ts,
            });
        }
        Ok(out)
    }

    /// All views, newest first.
    pub fn list_views(&self) -> Result<Vec<View>> {
        let mut stmt = self.conn.prepare(
            "SELECT view_id, base_change_id, applied_stacks, state_hash
             FROM views ORDER BY rowid DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(View {
                view_id: row.get(0)?,
                base_change_id: row.get(1)?,
                applied_stacks: row.get(2)?,
                state_hash: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Most-recently opened view (for /active-view compatibility with Vite plugin).
    pub fn latest_view(&self) -> Result<Option<View>> {
        self.conn
            .query_row(
                "SELECT view_id, base_change_id, applied_stacks, state_hash
                 FROM views ORDER BY rowid DESC LIMIT 1",
                [],
                |row| {
                    Ok(View {
                        view_id: row.get(0)?,
                        base_change_id: row.get(1)?,
                        applied_stacks: row.get(2)?,
                        state_hash: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Expose the path where the store lives (for status endpoint).
    pub fn store_path(&self) -> &std::path::Path {
        &self.root
    }

    /// Build a complete wire bundle for remote push/pull.
    pub fn export_bundle(&self, project_id: &str) -> Result<HubBundle> {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

        let stacks = self
            .list_stacks()?
            .into_iter()
            .map(|s| HubStack {
                stack_id: s.stack_id,
                agent_id: s.agent_id,
                base_change_id: s.base_change_id,
                tip_change_id: s.tip_change_id,
                status: s.status.to_string(),
            })
            .collect::<Vec<_>>();

        let changes = self
            .list_changes()?
            .into_iter()
            .map(|c| HubChange {
                change_id: c.change_id,
                parent_id: c.parent_id,
                path: c.path,
                op: c.op.to_string(),
                diff_hash: c.diff_hash,
                agent_id: c.agent_id,
                reason: c.intent.reason,
                task_ref: c.intent.task_ref,
                created_at: c.created_at,
            })
            .collect::<Vec<_>>();

        let files = self.list_file_entries()?;
        let edits = self.list_edit_metadata()?;
        let mut blob_hashes = BTreeSet::new();
        for change in &changes {
            if let Some(hash) = &change.diff_hash {
                blob_hashes.insert(hash.clone());
            }
        }
        for entry in &files {
            if let Some(hash) = &entry.blob_hash {
                blob_hashes.insert(hash.clone());
            }
        }
        for edit in &edits {
            if let Some(hash) = &edit.base_blob_hash {
                blob_hashes.insert(hash.clone());
            }
            if let Some(hash) = &edit.result_blob_hash {
                blob_hashes.insert(hash.clone());
            }
            if let Some(hash) = &edit.patch_blob_hash {
                blob_hashes.insert(hash.clone());
            }
        }

        let mut blobs = HashMap::new();
        for hash in blob_hashes {
            let data = self.blobs.get(&hash)?;
            blobs.insert(hash, B64.encode(data));
        }

        Ok(HubBundle {
            project_id: project_id.to_owned(),
            stacks,
            changes,
            files,
            edits,
            blobs,
        })
    }

    /// Ingest a [`HubBundle`] from a remote project.
    ///
    /// Idempotent — uses `INSERT OR IGNORE` so re-pushing the same bundle
    /// is a no-op (content-addressed blobs and deterministic change IDs).
    pub fn import_bundle(&self, bundle: &HubBundle) -> Result<(usize, usize, usize)> {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

        // 1. Blobs
        let mut blob_count = 0usize;
        for (hash, b64) in &bundle.blobs {
            let data = B64
                .decode(b64)
                .map_err(|e| VcsError::Other(format!("blob {hash} base64: {e}")))?;
            let actual = self.blobs.put(&data)?;
            if actual != *hash {
                return Err(VcsError::Other(format!(
                    "blob hash mismatch: sent {hash}, stored {actual}"
                )));
            }
            blob_count += 1;
        }

        // 2. Stacks (OR IGNORE — already present stacks are skipped)
        for s in &bundle.stacks {
            self.conn.execute(
                "INSERT OR IGNORE INTO stacks
                 (stack_id, agent_id, base_change_id, tip_change_id, status)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    s.stack_id,
                    s.agent_id,
                    s.base_change_id,
                    s.tip_change_id,
                    s.status
                ],
            )?;
        }

        // 3. Changes (OR IGNORE)
        for c in &bundle.changes {
            let intent_json = serde_json::json!({
                "reason":   c.reason,
                "task_ref": c.task_ref,
            })
            .to_string();
            self.conn.execute(
                "INSERT OR IGNORE INTO changes
                 (change_id, parent_id, path, op, diff_hash, agent_id, intent, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    c.change_id,
                    c.parent_id,
                    c.path,
                    c.op,
                    c.diff_hash,
                    c.agent_id,
                    intent_json,
                    c.created_at
                ],
            )?;
        }

        // 4. Derived file-state index rows.
        for f in &bundle.files {
            self.conn.execute(
                "INSERT OR REPLACE INTO files_at_change (change_id, path, blob_hash)
                 VALUES (?1,?2,?3)",
                params![f.change_id, f.path, f.blob_hash],
            )?;
        }

        // 5. Structured edit metadata.
        for e in &bundle.edits {
            self.conn.execute(
                "INSERT OR REPLACE INTO edit_metadata
                 (change_id, path, base_blob_hash, result_blob_hash, patch_blob_hash,
                  edit_kind, start_line, end_line, inserted_lines, deleted_lines)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    e.change_id,
                    e.path,
                    e.base_blob_hash,
                    e.result_blob_hash,
                    e.patch_blob_hash,
                    e.edit_kind,
                    e.start_line,
                    e.end_line,
                    e.inserted_lines,
                    e.deleted_lines,
                ],
            )?;
        }

        Ok((blob_count, bundle.stacks.len(), bundle.changes.len()))
    }
}

// ── private helpers ────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffEntry {
    pub path: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}

/// Entry in a chain-walk diff (`vcs diff <from> <to>`).
/// op is one of: "create" | "edit" | "delete"
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChainDiffEntry {
    pub path: String,
    pub op: String,
    pub blob_hash: Option<String>,
}

impl Store {
    fn get_view(&self, view_id: &str) -> Result<View> {
        self.conn
            .query_row(
                "SELECT view_id, base_change_id, applied_stacks, state_hash
                 FROM views WHERE view_id=?1",
                params![view_id],
                |row| {
                    Ok(View {
                        view_id: row.get(0)?,
                        base_change_id: row.get(1)?,
                        applied_stacks: row.get(2)?,
                        state_hash: row.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| VcsError::ViewNotFound(view_id.to_owned()))
    }

    fn get_change(&self, change_id: &str) -> Result<Change> {
        self.conn
            .query_row(
                "SELECT change_id, parent_id, path, op, diff_hash, agent_id, intent, created_at
                 FROM changes WHERE change_id=?1",
                params![change_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()?
            .map(|(cid, pid, path, op_s, dh, aid, intent_s, ts)| {
                Ok::<Change, VcsError>(Change {
                    change_id: cid,
                    parent_id: pid,
                    path,
                    op: op_s.parse()?,
                    diff_hash: dh,
                    agent_id: aid,
                    intent: Intent::from_json(&intent_s)?,
                    created_at: ts,
                })
            })
            .transpose()?
            .ok_or_else(|| VcsError::ChangeNotFound(change_id.to_owned()))
    }

    /// Final file state produced by a single stack (path → blob_hash|None).
    fn stack_snapshot(&self, stack_id: &str) -> Result<HashMap<String, Option<String>>> {
        let stk = self.get_stack(stack_id)?;
        let Some(tip) = stk.tip_change_id else {
            return Ok(HashMap::new());
        };

        // Walk change chain tip→base, collecting the LATEST state per path.
        // We fetch all files_at_change rows per change (not just the change.path
        // field) so that rename — which writes TWO rows: old=NULL, new=hash —
        // is handled correctly.
        let mut out: HashMap<String, Option<String>> = HashMap::new();
        let mut current = Some(tip);

        while let Some(cid) = current {
            if stk.base_change_id.as_deref() == Some(cid.as_str()) {
                break;
            }
            let change = self.get_change(&cid)?;

            for entry in self.file_entries_for_change(&cid)? {
                // First occurrence while walking tip→base is the latest value
                out.entry(entry.path).or_insert(entry.blob_hash);
            }

            current = change.parent_id.clone();
        }
        Ok(out)
    }

    /// The blob hash for a file in a specific stack (None = deleted/not present).
    fn stack_file_blob(&self, stack_id: &str, path: &str) -> Result<Option<String>> {
        let snap = self.stack_snapshot(stack_id)?;
        Ok(snap.get(path).cloned().flatten())
    }

    fn build_stack_snapshots(
        &self,
        stacks: &[StackId],
    ) -> Result<Vec<(StackId, HashMap<String, Option<String>>)>> {
        stacks
            .iter()
            .map(|sid| {
                let snap = self.stack_snapshot(sid)?;
                Ok((sid.clone(), snap))
            })
            .collect()
    }

    /// Enhanced merge that also records which change_id produced each version.
    fn compute_merge_with_candidates(
        &self,
        base_snap: &HashMap<String, String>,
        stack_snaps: &[(StackId, HashMap<String, Option<String>>)],
    ) -> Result<MergedTreeWithCandidates> {
        let mut clean: HashMap<String, Option<String>> = HashMap::new();
        let mut conflicts: HashMap<String, Vec<Candidate>> = HashMap::new();
        let mut auto_resolved: HashMap<String, (Vec<Candidate>, Resolution)> = HashMap::new();
        let mut touched: HashMap<String, Vec<(StackId, Option<String>)>> = HashMap::new();

        for (sid, snap) in stack_snaps {
            for (path, blob) in snap {
                touched
                    .entry(path.clone())
                    .or_default()
                    .push((sid.clone(), blob.clone()));
            }
        }

        for (path, writers) in &touched {
            if writers.len() == 1 {
                clean.insert(path.clone(), writers[0].1.clone());
                continue;
            }

            // ── P0.2 / P1.2: content-aware conflict detection ────────────────

            // Case 1: all stacks have the same blob hash → no conflict
            let unique_blobs: std::collections::HashSet<Option<&str>> = writers
                .iter()
                .map(|(_, blob)| blob.as_deref())
                .collect();
            if unique_blobs.len() == 1 {
                clean.insert(path.clone(), writers[0].1.clone());
                continue;
            }

            // Case 2: exactly one stack actually modified the file relative to
            // the base — the others coincidentally have the same hash as base
            // (or the base is empty and only one stack created the file).
            let base_hash: Option<&str> = base_snap.get(path).map(String::as_str);
            let modified: Vec<_> = writers
                .iter()
                .filter(|(_, blob)| blob.as_deref() != base_hash)
                .collect();
            if modified.len() == 1 {
                // Only one stack changed it → use that version, no conflict
                clean.insert(path.clone(), modified[0].1.clone());
                continue;
            }

            // Case 3: 2+ stacks modified the file differently → build candidates
            // and attempt non-overlapping line-level 3-way merge.
            let mut candidates = Vec::new();
            for (sid, blob) in writers {
                let tip_cid = self
                    .latest_change_for_path_in_stack(sid, path)?
                    .unwrap_or_else(|| {
                        self.get_stack(sid)
                            .ok()
                            .and_then(|s| s.tip_change_id)
                            .unwrap_or_default()
                    });
                candidates.push(Candidate {
                    stack_id: sid.clone(),
                    change_id: tip_cid,
                    blob_hash: blob.clone(),
                });
            }

            if let Some(merged_blob) =
                self.try_merge_non_overlapping_edits(path, &candidates)?
            {
                clean.insert(path.clone(), Some(merged_blob.clone()));
                auto_resolved.insert(
                    path.clone(),
                    (
                        candidates,
                        Resolution::Merge {
                            blob_hash: merged_blob,
                        },
                    ),
                );
            } else {
                conflicts.insert(path.clone(), candidates);
            }
        }

        // Inherit base paths not touched by any stack
        for (path, hash) in base_snap {
            if !touched.contains_key(path) {
                clean.insert(path.clone(), Some(hash.clone()));
            }
        }

        Ok(MergedTreeWithCandidates {
            clean,
            conflicts,
            auto_resolved,
        })
    }

    fn latest_change_for_path_in_stack(
        &self,
        stack_id: &str,
        path: &str,
    ) -> Result<Option<ChangeId>> {
        let stk = self.get_stack(stack_id)?;
        let mut current = stk.tip_change_id;
        while let Some(cid) = current {
            if stk.base_change_id.as_deref() == Some(cid.as_str()) {
                break;
            }
            let entries = self.file_entries_for_change(&cid)?;
            if entries.iter().any(|entry| entry.path == path) {
                return Ok(Some(cid));
            }
            current = self.get_change(&cid)?.parent_id;
        }
        Ok(None)
    }

    fn try_merge_non_overlapping_edits(
        &self,
        path: &str,
        candidates: &[Candidate],
    ) -> Result<Option<String>> {
        let mut edits = Vec::new();
        let mut base_hash: Option<String> = None;

        for candidate in candidates {
            let Some(meta) = self.get_edit_metadata(&candidate.change_id)? else {
                return Ok(None);
            };
            if meta.path != path || meta.edit_kind != EditKind::ReplaceLines {
                return Ok(None);
            }
            let Some(meta_base) = meta.base_blob_hash.clone() else {
                return Ok(None);
            };
            if base_hash.as_deref().is_some_and(|known| known != meta_base) {
                return Ok(None);
            }
            let (Some(start), Some(end), Some(patch_hash)) =
                (meta.start_line, meta.end_line, meta.patch_blob_hash.clone())
            else {
                return Ok(None);
            };
            base_hash = Some(meta_base);
            edits.push((start, end, patch_hash));
        }

        if ranges_overlap(&edits) {
            return Ok(None);
        }

        let Some(base_hash) = base_hash else {
            return Ok(None);
        };
        let mut lines = split_lines_keepends(&self.blobs.get(&base_hash)?);

        edits.sort_by(|a, b| b.0.cmp(&a.0));
        for (start, end, patch_hash) in edits {
            let patch: EditPatch = serde_json::from_slice(&self.blobs.get(&patch_hash)?)?;
            let EditPatch::ReplaceLines {
                replacement_b64, ..
            } = patch
            else {
                return Ok(None);
            };
            let replacement = decode_b64(&replacement_b64)?;
            let replacement_lines = split_lines_keepends(&replacement);
            lines.splice(start as usize..end as usize, replacement_lines);
        }

        Ok(Some(self.blobs.put(&lines.concat())?))
    }

    fn files_at_change_id(&self, change_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM files_at_change WHERE change_id=?1 AND blob_hash IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![change_id], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn file_entries_for_change(&self, change_id: &str) -> Result<Vec<HubFileEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT change_id, path, blob_hash FROM files_at_change WHERE change_id=?1")?;
        let rows = stmt.query_map(params![change_id], |r| {
            Ok(HubFileEntry {
                change_id: r.get(0)?,
                path: r.get(1)?,
                blob_hash: r.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn list_file_entries(&self) -> Result<Vec<HubFileEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT change_id, path, blob_hash FROM files_at_change ORDER BY change_id, path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(HubFileEntry {
                change_id: r.get(0)?,
                path: r.get(1)?,
                blob_hash: r.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_edit_metadata(&self) -> Result<Vec<HubEditMetadata>> {
        let mut stmt = self.conn.prepare(
            "SELECT change_id, path, base_blob_hash, result_blob_hash, patch_blob_hash,
                    edit_kind, start_line, end_line, inserted_lines, deleted_lines
             FROM edit_metadata ORDER BY change_id, path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(HubEditMetadata {
                change_id: r.get(0)?,
                path: r.get(1)?,
                base_blob_hash: r.get(2)?,
                result_blob_hash: r.get(3)?,
                patch_blob_hash: r.get(4)?,
                edit_kind: r.get(5)?,
                start_line: r.get(6)?,
                end_line: r.get(7)?,
                inserted_lines: r.get(8)?,
                deleted_lines: r.get(9)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_edit_metadata(&self, change_id: &str) -> Result<Option<EditMetadata>> {
        self.conn
            .query_row(
                "SELECT change_id, path, base_blob_hash, result_blob_hash, patch_blob_hash,
                        edit_kind, start_line, end_line, inserted_lines, deleted_lines
                 FROM edit_metadata WHERE change_id=?1",
                params![change_id],
                |r| {
                    let kind: String = r.get(5)?;
                    Ok(EditMetadata {
                        change_id: r.get(0)?,
                        path: r.get(1)?,
                        base_blob_hash: r.get(2)?,
                        result_blob_hash: r.get(3)?,
                        patch_blob_hash: r.get(4)?,
                        edit_kind: kind.parse().unwrap_or(EditKind::ReplaceLines),
                        start_line: r.get(6)?,
                        end_line: r.get(7)?,
                        inserted_lines: r.get(8)?,
                        deleted_lines: r.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

struct MergedTreeWithCandidates {
    clean: HashMap<String, Option<String>>,
    conflicts: HashMap<String, Vec<Candidate>>,
    auto_resolved: HashMap<String, (Vec<Candidate>, Resolution)>,
}

struct EditPlan {
    patch: EditPatch,
    edit_kind: EditKind,
    start_line: Option<u32>,
    end_line: Option<u32>,
    inserted_lines: u32,
    deleted_lines: u32,
}

fn build_edit_patch(
    base_blob_hash: Option<String>,
    base_content: Option<&[u8]>,
    new_content: &[u8],
) -> EditPlan {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

    let Some(base_content) = base_content else {
        return EditPlan {
            patch: EditPatch::Create {
                content_b64: B64.encode(new_content),
                result_blob_hash: crate::blob::blake3_hex(new_content),
            },
            edit_kind: EditKind::Create,
            start_line: Some(0),
            end_line: Some(0),
            inserted_lines: split_lines_keepends(new_content).len() as u32,
            deleted_lines: 0,
        };
    };

    let base_lines = split_lines_keepends(base_content);
    let new_lines = split_lines_keepends(new_content);
    let mut prefix = 0usize;
    while prefix < base_lines.len()
        && prefix < new_lines.len()
        && base_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix < base_lines.len().saturating_sub(prefix)
        && suffix < new_lines.len().saturating_sub(prefix)
        && base_lines[base_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let base_end = base_lines.len() - suffix;
    let new_end = new_lines.len() - suffix;
    let replacement = new_lines[prefix..new_end].concat();
    let inserted_lines = new_end.saturating_sub(prefix) as u32;
    let deleted_lines = base_end.saturating_sub(prefix) as u32;

    EditPlan {
        patch: EditPatch::ReplaceLines {
            base_blob_hash: base_blob_hash.unwrap_or_default(),
            result_blob_hash: crate::blob::blake3_hex(new_content),
            start_line: prefix as u32,
            end_line: base_end as u32,
            replacement_b64: B64.encode(replacement),
        },
        edit_kind: EditKind::ReplaceLines,
        start_line: Some(prefix as u32),
        end_line: Some(base_end as u32),
        inserted_lines,
        deleted_lines,
    }
}

fn split_lines_keepends(data: &[u8]) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (idx, byte) in data.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(data[start..=idx].to_vec());
            start = idx + 1;
        }
    }
    if start < data.len() {
        lines.push(data[start..].to_vec());
    }
    lines
}

fn ranges_overlap(edits: &[(u32, u32, String)]) -> bool {
    let mut ranges = edits
        .iter()
        .map(|(start, end, _)| (*start, *end))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        let (_, prev_end) = pair[0];
        let (next_start, _) = pair[1];
        if prev_end > next_start {
            return true;
        }
    }
    false
}

fn decode_b64(input: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    B64.decode(input)
        .map_err(|e| VcsError::Other(format!("patch base64: {e}")))
}

// ── Session tracking ───────────────────────────────────────────────────────

impl Store {
    /// Register a new agent session. Call at the start of every Claude Code chat.
    ///
    /// Sessions are data in the store — they don't touch the filesystem.
    /// When the agent is ready to test, it calls `vcs checkout <view_id> <dir>`
    /// to materialise its stack to any directory it chooses.  Two sessions pick
    /// different output dirs; two dev-servers start there on different ports.
    /// The store (`.vcs/`) is shared — WAL mode handles concurrent access safely.
    ///
    /// `port` is optional metadata so `vcs_overview` can show which port each
    /// session's dev-server is expected to run on.
    pub fn session_open(&self, agent_id: &str, port: Option<u16>) -> Result<String> {
        let session_id = new_id();
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO sessions
             (session_id, agent_id, stack_id, started_at, last_seen_at, status, phase, worktree, port)
             VALUES (?1, ?2, NULL, ?3, ?3, 'active', 'working', NULL, ?4)",
            params![session_id, agent_id, now, port.map(|p| p as i64)],
        )?;
        tracing::debug!(%session_id, %agent_id, "session opened");
        Ok(session_id)
    }

    /// Record the output directory this session is currently serving from.
    /// Call after `vcs checkout` so `vcs_overview` can show each session's path/port.
    pub fn session_set_output(&self, session_id: &str, output_dir: &str, port: Option<u16>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET worktree=?1, port=?2, last_seen_at=?3 WHERE session_id=?4",
            params![output_dir, port.map(|p| p as i64), now_ms(), session_id],
        )?;
        Ok(())
    }

    /// Link a stack to a session after `open_stack` is called.
    pub fn session_link_stack(&self, session_id: &str, stack_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET stack_id=?1, last_seen_at=?2 WHERE session_id=?3",
            params![stack_id, now_ms(), session_id],
        )?;
        Ok(())
    }

    /// Heartbeat — update last_seen_at to prove the session is still alive.
    pub fn session_heartbeat(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET last_seen_at=?1 WHERE session_id=?2",
            params![now_ms(), session_id],
        )?;
        Ok(())
    }

    /// Set the session phase.
    ///
    /// - `working`  — still making changes (default)
    /// - `testing`  — changes done; running tests / dev server; blocks merges
    /// - `done`     — validated; ready for merge
    ///
    /// Only one session should be in `testing` phase at a time.
    /// Other sessions MUST NOT merge until the testing session reaches `done`.
    pub fn session_set_phase(&self, session_id: &str, phase: &str) -> Result<()> {
        match phase {
            "working" | "testing" | "done" => {}
            other => return Err(VcsError::Other(
                format!("unknown phase '{other}' — use working|testing|done")
            )),
        }
        self.conn.execute(
            "UPDATE sessions SET phase=?1, last_seen_at=?2 WHERE session_id=?3",
            params![phase, now_ms(), session_id],
        )?;
        tracing::debug!(%session_id, %phase, "session phase changed");
        Ok(())
    }

    /// Get a single session by ID.
    pub fn get_session(&self, session_id: &str) -> Result<Session> {
        self.conn.query_row(
            "SELECT session_id, agent_id, stack_id, started_at, last_seen_at, status, phase, worktree, port
             FROM sessions WHERE session_id=?1",
            params![session_id],
            |r| Ok(Session {
                session_id:   r.get(0)?,
                agent_id:     r.get(1)?,
                stack_id:     r.get(2)?,
                started_at:   r.get(3)?,
                last_seen_at: r.get(4)?,
                status:       r.get(5)?,
                phase:        r.get(6)?,
                worktree:     r.get(7)?,
                port:         r.get::<_, Option<i64>>(8)?.map(|p| p as u16),
            }),
        )
        .optional()?
        .ok_or_else(|| VcsError::Other(format!("session {session_id} not found")))
    }

    /// Mark session as done. The associated stack stays open for future merging.
    pub fn session_close(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET status='done', phase='done', last_seen_at=?1 WHERE session_id=?2",
            params![now_ms(), session_id],
        )?;
        tracing::debug!(%session_id, "session closed");
        Ok(())
    }

    /// List all sessions, newest first.
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, agent_id, stack_id, started_at, last_seen_at, status, phase, worktree, port
             FROM sessions ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Session {
                session_id:   r.get(0)?,
                agent_id:     r.get(1)?,
                stack_id:     r.get(2)?,
                started_at:   r.get(3)?,
                last_seen_at: r.get(4)?,
                status:       r.get(5)?,
                phase:        r.get(6)?,
                worktree:     r.get(7)?,
                port:         r.get::<_, Option<i64>>(8)?.map(|p| p as u16),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Check which OTHER open stacks have also touched `path`.
    /// Returns an empty vec if no contention — call after every `edit`.
    pub fn file_contention(&self, path: &str, caller_stack_id: &str) -> Result<FileContention> {
        // Walk every open stack's full change chain (tip→base) to check whether
        // the file appears anywhere, not just in the tip change.
        // (The tip-only JOIN missed files edited in earlier changes of the stack.)
        let open_stacks = self.list_stacks()?.into_iter()
            .filter(|s| s.status == crate::stack::StackStatus::Open
                     && s.stack_id != caller_stack_id)
            .collect::<Vec<_>>();

        let mut other_stacks = Vec::new();
        for stk in open_stacks {
            let snap = self.stack_snapshot(&stk.stack_id).unwrap_or_default();
            // blob_hash = Some(hash) means file exists; None means deleted/not present
            if matches!(snap.get(path), Some(Some(_))) {
                // Find the change_id where this path was most recently touched
                let change_id = snap.keys()
                    .find(|p| p.as_str() == path)
                    .and_then(|_| {
                        // Walk chain to find the first (most recent) change that touched path
                        let mut cur = stk.tip_change_id.clone();
                        while let Some(ref cid) = cur {
                            if stk.base_change_id.as_deref() == Some(cid.as_str()) { break; }
                            let entries = self.file_entries_for_change(cid).unwrap_or_default();
                            if entries.iter().any(|e| e.path == path && e.blob_hash.is_some()) {
                                return Some(cid.clone());
                            }
                            cur = self.get_change(cid).ok().and_then(|c| c.parent_id);
                        }
                        None
                    })
                    .unwrap_or_default();

                other_stacks.push(ContentionEntry {
                    stack_id:  stk.stack_id.clone(),
                    agent_id:  stk.agent_id.clone(),
                    change_id,
                });
            }
        }

        Ok(FileContention {
            path: path.to_owned(),
            other_stacks,
        })
    }

    /// Build a full multi-agent overview — the primary tool for Claude to narrate
    /// what every agent is doing without a human opening a browser.
    pub fn overview(&self) -> Result<AgentOverview> {
        let now = now_ms();

        let sessions_raw = self.list_sessions()?;
        let open_stacks = self.list_stacks()?.into_iter()
            .filter(|s| s.status == crate::stack::StackStatus::Open)
            .collect::<Vec<_>>();

        // Identify any session currently in "testing" phase
        let testing_session = sessions_raw.iter()
            .find(|s| s.phase == "testing" && s.status == "active")
            .map(|s| s.agent_id.clone());

        // Build per-session summaries
        let mut summaries: Vec<SessionSummary> = Vec::new();
        for sess in &sessions_raw {
            let (files_touched, changes_count) = if let Some(ref sid) = sess.stack_id {
                let snap = self.stack_snapshot(sid).unwrap_or_default();
                let files = snap.keys().cloned().collect::<Vec<_>>();
                let count = self.log(sid).unwrap_or_default().len();
                (files, count)
            } else {
                (vec![], 0)
            };
            summaries.push(SessionSummary {
                session_id:    sess.session_id.clone(),
                agent_id:      sess.agent_id.clone(),
                stack_id:      sess.stack_id.clone(),
                status:        sess.status.clone(),
                phase:         sess.phase.clone(),
                worktree:      sess.worktree.clone(),
                port:          sess.port,
                files_touched,
                changes_count,
                started_at:    sess.started_at,
                last_seen_at:  sess.last_seen_at,
            });
        }

        // Find hot files: paths touched by 2+ open stacks.
        // Track (agent_id, blob_hash) so we can correctly set will_conflict.
        let mut path_to_entries: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for stk in &open_stacks {
            let snap = self.stack_snapshot(&stk.stack_id).unwrap_or_default();
            for (path, blob) in snap {
                if let Some(hash) = blob {
                    path_to_entries
                        .entry(path)
                        .or_default()
                        .push((stk.agent_id.clone(), hash));
                }
            }
        }
        let mut hot_files: Vec<crate::session::HotFile> = path_to_entries
            .into_iter()
            .filter(|(_, entries)| entries.len() > 1)
            .map(|(path, entries)| {
                let unique_hashes: std::collections::HashSet<&str> =
                    entries.iter().map(|(_, h)| h.as_str()).collect();
                let will_conflict = unique_hashes.len() > 1;
                let touched_by = entries.into_iter().map(|(a, _)| a).collect();
                crate::session::HotFile { will_conflict, path, touched_by }
            })
            .collect();
        hot_files.sort_by(|a, b| a.path.cmp(&b.path));

        let active_count = sessions_raw.iter().filter(|s| s.status == "active").count();
        let summary = build_overview_summary(
            active_count, &sessions_raw, &open_stacks, &hot_files, testing_session.as_deref()
        );

        Ok(AgentOverview {
            sessions: summaries,
            hot_files,
            active_count,
            testing_session,
            summary,
            generated_at: now,
        })
    }
}

fn build_overview_summary(
    active_count: usize,
    sessions: &[Session],
    open_stacks: &[crate::stack::Stack],
    hot_files: &[crate::session::HotFile],
    testing_session: Option<&str>,
) -> String {
    if active_count == 0 && open_stacks.is_empty() {
        return "No active sessions. Store is idle.".into();
    }

    let mut lines: Vec<String> = Vec::new();

    lines.push(format!(
        "{active_count} active session(s), {} open stack(s).",
        open_stacks.len()
    ));

    // Show each session with phase + output dir
    for s in sessions.iter().filter(|s| s.status == "active") {
        let phase_icon = match s.phase.as_str() {
            "testing" => "🧪",
            "done"    => "✓",
            _         => "✏️ ",
        };
        let output = s.worktree.as_deref().unwrap_or("(not checked out yet)");
        let port_str = s.port.map(|p| format!(" → port {p}")).unwrap_or_default();
        lines.push(format!(
            "  {phase_icon} {} [{}] — output: {output}{port_str}",
            s.agent_id, s.phase
        ));
    }

    // Gate: warn if a testing session blocks merges
    if let Some(tester) = testing_session {
        lines.push(format!("\n⛔ {tester} is TESTING — other sessions must NOT merge until this session reaches phase=done."));
    }

    if hot_files.is_empty() {
        lines.push("  ✓ No file conflicts between open stacks.".into());
    } else {
        lines.push(format!("  ⚡ {} file(s) will conflict when merged:", hot_files.len()));
        for hf in hot_files {
            lines.push(format!(
                "    - {} → touched by: {}",
                hf.path,
                hf.touched_by.join(", ")
            ));
        }
    }

    lines.join("\n")
}
