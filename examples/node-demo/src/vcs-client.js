/**
 * vcs-client.js
 *
 * Thin Node.js wrapper around the `vcs` CLI binary.
 * Every method maps 1:1 to a CLI subcommand and returns parsed JSON.
 *
 * The CLI binary is resolved in order:
 *   1. VCS_BIN env var
 *   2. ../target/release/vcs  (sibling cargo workspace)
 *   3. vcs on PATH
 */

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const __dir = dirname(fileURLToPath(import.meta.url));

function findBin() {
  if (process.env.VCS_BIN) return process.env.VCS_BIN;
  const rel = resolve(__dir, '../../target/release/vcs');
  if (existsSync(rel)) return rel;
  return 'vcs'; // hope it's on PATH
}

const BIN = findBin();

export class VcsClient {
  /**
   * @param {object} opts
   * @param {string} [opts.storePath]  – path to the vcs store directory
   * @param {boolean} [opts.verbose]   – log raw CLI invocations
   */
  constructor({ storePath, verbose = false } = {}) {
    this.storePath = storePath ?? join(tmpdir(), `vcs-${process.pid}`);
    this.verbose = verbose;
  }

  // ── internal ─────────────────────────────────────────────────────────────

  _run(args, input) {
    const baseArgs = ['--store', this.storePath, '--json', ...args];
    if (this.verbose) {
      process.stderr.write(`[vcs] ${BIN} ${baseArgs.join(' ')}\n`);
    }

    const result = spawnSync(BIN, baseArgs, {
      input,
      encoding: 'utf8',
      maxBuffer: 32 * 1024 * 1024,
    });

    if (result.status !== 0) {
      throw new Error(
        `vcs ${args[0]} failed (exit ${result.status}):\n${result.stderr}`,
      );
    }

    const out = result.stdout.trim();
    if (!out) return null;
    try {
      return JSON.parse(out);
    } catch {
      return out;
    }
  }

  // ── store ─────────────────────────────────────────────────────────────────

  /** Initialise the store.  Idempotent (open_or_init). */
  init() {
    return this._run(['init']);
  }

  // ── stacks ────────────────────────────────────────────────────────────────

  /** Open a new stack for agentId, optionally on top of baseChangeId. */
  stackOpen(agentId, baseChangeId) {
    const args = ['stack', 'open', '--agent', agentId];
    if (baseChangeId) args.push('--base', baseChangeId);
    return this._run(args).stack_id;
  }

  stackClose(stackId) {
    return this._run(['stack', 'close', stackId]);
  }

  stackAbandon(stackId) {
    return this._run(['stack', 'abandon', stackId]);
  }

  stackInfo(stackId) {
    return this._run(['stack', 'info', stackId]);
  }

  // ── edits ─────────────────────────────────────────────────────────────────

  /**
   * Record an edit.
   * @param {string} stackId
   * @param {string} path
   * @param {string|Buffer} content
   * @param {object} intent  – { reason, task_ref?, tool_call? }
   * @returns {string} changeId
   */
  edit(stackId, path, content, { reason, task_ref, tool_call } = {}) {
    if (!reason) throw new Error('intent.reason is required');

    // Write content to a temp file so we avoid shell escaping hell
    const tmp = join(tmpdir(), `vcs-edit-${Date.now()}-${Math.random().toString(36).slice(2)}`);
    writeFileSync(tmp, content);

    const args = [
      'edit', stackId, path,
      '--content-file', tmp,
      '--reason', reason,
    ];
    if (task_ref) args.push('--task-ref', task_ref);
    if (tool_call) args.push('--tool-call', JSON.stringify(tool_call));

    try {
      return this._run(args).change_id;
    } finally {
      try { require('fs').unlinkSync(tmp); } catch {}
    }
  }

  /**
   * Record a deletion.
   * @returns {string} changeId
   */
  delete(stackId, path, { reason, task_ref } = {}) {
    if (!reason) throw new Error('intent.reason is required');
    const args = ['delete', stackId, path, '--reason', reason];
    if (task_ref) args.push('--task-ref', task_ref);
    return this._run(args).change_id;
  }

  // ── views ─────────────────────────────────────────────────────────────────

  /** Open a view over baseChangeId with the given stacks applied. */
  viewOpen(baseChangeId, stackIds) {
    return this._run([
      'view', 'open',
      '--base', baseChangeId,
      '--stacks', stackIds.join(','),
    ]).view_id;
  }

  /** Read a file's content as a string. */
  viewRead(viewId, path) {
    return this._run(['view', 'read', viewId, path]);
  }

  /** List all files visible in a view. */
  viewLs(viewId) {
    return this._run(['view', 'ls', viewId]).files;
  }

  /** Return conflict objects for a view. */
  viewConflicts(viewId) {
    return this._run(['view', 'conflicts', viewId]);
  }

  /**
   * Resolve a conflict by picking a stack.
   */
  resolveByPick(conflictId, stackId) {
    return this._run(['view', 'resolve', conflictId, '--pick', stackId]);
  }

  /**
   * Resolve a conflict by providing merged content.
   */
  resolveByMerge(conflictId, mergedContent) {
    const tmp = join(tmpdir(), `vcs-merge-${Date.now()}`);
    writeFileSync(tmp, mergedContent);
    return this._run(['view', 'resolve', conflictId, '--merge-file', tmp]);
  }

  // ── inspection ────────────────────────────────────────────────────────────

  /** Return the change log for a stack as an array of Change objects. */
  log(stackId) {
    return this._run(['log', stackId]);
  }

  /** Diff two change IDs. */
  diff(from, to) {
    return this._run(['diff', from, to]);
  }
}

// ── Convenience factory ────────────────────────────────────────────────────

/**
 * Create a VcsClient backed by a fresh temp directory.
 * Useful for tests.
 */
export function tempStore(verbose = false) {
  const dir = mkdtempSync(join(tmpdir(), 'vcs-test-'));
  const client = new VcsClient({ storePath: dir, verbose });
  client.init();
  return client;
}
