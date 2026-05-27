//! Store — the single entry-point for all VCS operations.
//!
//! Layout on disk:
//!   <root>/vcs.db          SQLite database
//!   <root>/blobs/          content-addressed blob store

use crate::blob::BlobStore;
use crate::change::{compute_change_id, Change, ChangeId, ConflictId, Op, StackId, ViewId};
use crate::error::{Result, VcsError};
use crate::intent::Intent;
use crate::stack::{Stack, StackStatus};
use crate::view::{state_hash, Candidate, Conflict, Resolution, View};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = include_str!("schema.sql");

pub struct Store {
    conn:  Connection,
    blobs: BlobStore,
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
        Ok(Self { conn, blobs })
    }

    /// Open an existing store at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let db_path = path.join("vcs.db");
        if !db_path.exists() {
            return Err(VcsError::NotInitialised(path.display().to_string()));
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let blobs = BlobStore::new(path)?;
        Ok(Self { conn, blobs })
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
                        stack_id:       row.get(0)?,
                        agent_id:       row.get(1)?,
                        base_change_id: row.get(2)?,
                        tip_change_id:  row.get(3)?,
                        status:         {
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

        // Store the content blob
        let blob_hash = self.blobs.put(new_content)?;

        // Determine op: create if the file didn't exist at base, edit otherwise
        let op = if self.file_exists_at_base(&stk, path)? {
            Op::Edit
        } else {
            Op::Create
        };

        // Also store a trivial diff blob (full content for now; a real impl
        // would compute a line diff here — noted as extension point)
        let diff_hash = blob_hash.clone();

        let parent_id = stk.tip_change_id.as_deref();
        let ts = now_ms();
        let change_id = compute_change_id(parent_id, path, Some(&diff_hash), &stk.agent_id, ts);

        self.insert_change(&change_id, parent_id, path, &op, Some(&diff_hash), &stk.agent_id, &intent, ts)?;
        self.upsert_files_at_change(&change_id, path, Some(&blob_hash))?;
        self.advance_stack_tip(stack, &change_id)?;

        tracing::debug!(%change_id, %path, op=%op, "edit recorded");
        Ok(change_id)
    }

    /// Record deletion of `path`.
    pub fn delete(&self, stack: &StackId, path: &str, intent: Intent) -> Result<ChangeId> {
        let stk = self.require_open_stack(stack)?;
        let parent_id = stk.tip_change_id.as_deref();
        let ts = now_ms();
        let change_id = compute_change_id(parent_id, path, None, &stk.agent_id, ts);

        self.insert_change(&change_id, parent_id, path, &Op::Delete, None, &stk.agent_id, &intent, ts)?;
        self.upsert_files_at_change(&change_id, path, None)?;
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
        let blob_hash = self.blobs.put(new_content)?;
        let path = format!("{from}\x00{to}"); // encode both paths in the path field
        let parent_id = stk.tip_change_id.as_deref();
        let ts = now_ms();
        let change_id =
            compute_change_id(parent_id, &path, Some(&blob_hash), &stk.agent_id, ts);

        self.insert_change(&change_id, parent_id, &path, &Op::Rename, Some(&blob_hash), &stk.agent_id, &intent, ts)?;
        // Delete old path, create new path in derived index
        self.upsert_files_at_change(&change_id, from, None)?;
        self.upsert_files_at_change(&change_id, to, Some(&blob_hash))?;
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
            return Err(VcsError::StackNotOpen(stack_id.to_owned(), s.status.to_string()));
        }
        Ok(s)
    }

    /// Does the file exist at the base of this stack (i.e., before any stack edits)?
    fn file_exists_at_base(&self, stk: &Stack, path: &str) -> Result<bool> {
        let Some(base) = &stk.base_change_id else {
            return Ok(false); // fresh repo — nothing exists
        };
        // Walk up from tip to find what the base snapshot looked like
        // Simplified: check files_at_change at base
        let exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files_at_change WHERE change_id=?1 AND path=?2 AND blob_hash IS NOT NULL",
                params![base, path],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);
        Ok(exists)
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
            None    => Err(VcsError::FileNotFound(path.to_owned())),
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
            out.push(Conflict { conflict_id, view_id, path, candidates, resolution });
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

    /// Return a simple diff summary between two change IDs.
    pub fn diff(&self, from: &ChangeId, to: &ChangeId) -> Result<Vec<DiffEntry>> {
        let from_snap = self.snapshot_at(from)?;
        let to_snap   = self.snapshot_at(to)?;

        let mut entries = Vec::new();
        let mut all_paths: std::collections::BTreeSet<&str> =
            from_snap.keys().map(|s| s.as_str()).collect();
        all_paths.extend(to_snap.keys().map(|s| s.as_str()));

        for path in all_paths {
            let before = from_snap.get(path).cloned();
            let after  = to_snap.get(path).cloned();
            if before != after {
                entries.push(DiffEntry {
                    path:        path.to_owned(),
                    before_hash: before,
                    after_hash:  after,
                });
            }
        }
        Ok(entries)
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
}

// ── private helpers ────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffEntry {
    pub path:        String,
    pub before_hash: Option<String>,
    pub after_hash:  Option<String>,
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
                        view_id:        row.get(0)?,
                        base_change_id: row.get(1)?,
                        applied_stacks: row.get(2)?,
                        state_hash:     row.get(3)?,
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
                    change_id:  cid,
                    parent_id:  pid,
                    path,
                    op:         op_s.parse()?,
                    diff_hash:  dh,
                    agent_id:   aid,
                    intent:     Intent::from_json(&intent_s)?,
                    created_at: ts,
                })
            })
            .transpose()?
            .ok_or_else(|| VcsError::ChangeNotFound(change_id.to_owned()))
    }

    /// The file tree state at a given change_id.
    fn snapshot_at(&self, change_id: &str) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, blob_hash FROM files_at_change
             WHERE change_id=?1 AND blob_hash IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![change_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (p, h) = r?;
            map.insert(p, h);
        }
        Ok(map)
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

            // Fetch every (path, blob_hash) row this change wrote
            let mut stmt = self.conn.prepare(
                "SELECT path, blob_hash FROM files_at_change WHERE change_id=?1",
            )?;
            let entries = stmt
                .query_map(params![cid], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            for (path, blob) in entries {
                // First occurrence while walking tip→base is the latest value
                out.entry(path).or_insert(blob);
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
        let mut touched: HashMap<String, Vec<(StackId, Option<String>)>> = HashMap::new();

        for (sid, snap) in stack_snaps {
            for (path, blob) in snap {
                touched.entry(path.clone()).or_default().push((sid.clone(), blob.clone()));
            }
        }

        for (path, writers) in &touched {
            if writers.len() == 1 {
                clean.insert(path.clone(), writers[0].1.clone());
            } else {
                // Build conflict candidates, find the tip change_id for each stack
                let mut candidates = Vec::new();
                for (sid, blob) in writers {
                    let tip_cid = self.get_stack(sid)?.tip_change_id.unwrap_or_default();
                    candidates.push(Candidate {
                        stack_id:  sid.clone(),
                        change_id: tip_cid,
                        blob_hash: blob.clone(),
                    });
                }
                conflicts.insert(path.clone(), candidates);
            }
        }

        // Inherit base paths not touched by any stack
        for (path, hash) in base_snap {
            if !touched.contains_key(path) {
                clean.insert(path.clone(), Some(hash.clone()));
            }
        }

        Ok(MergedTreeWithCandidates { clean, conflicts })
    }

    fn files_at_change_id(&self, change_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM files_at_change WHERE change_id=?1 AND blob_hash IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![change_id], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }
}

struct MergedTreeWithCandidates {
    clean:     HashMap<String, Option<String>>,
    conflicts: HashMap<String, Vec<Candidate>>,
}
