/**
 * agent-server.js
 *
 * A single "agent HTTP server" — an Express app that exposes the vcs store
 * operations over REST so external callers (the orchestrator) can drive it.
 *
 * Each agent server owns one agent identity and one vcs stack at a time.
 * The store is SHARED across all agent servers (path passed via --store or
 * env VCS_STORE_PATH), which is the key point: multiple servers, one data model.
 *
 * Usage:
 *   node agent-server.js --agent-id agent-alpha --port 3001 --store /tmp/vcs
 */

import express from 'express';
import { VcsClient } from './vcs-client.js';
import { parseArgs } from 'node:util';

const { values: argv } = parseArgs({
  options: {
    'agent-id': { type: 'string', default: `agent-${process.pid}` },
    port:       { type: 'string', default: '3000' },
    store:      { type: 'string' },
    verbose:    { type: 'boolean', default: false },
  },
  strict: false,
});

const AGENT_ID   = argv['agent-id'];
const PORT       = parseInt(argv.port, 10);
const STORE_PATH = argv.store ?? process.env.VCS_STORE_PATH;

if (!STORE_PATH) {
  console.error('ERROR: --store <path> or VCS_STORE_PATH env required');
  process.exit(1);
}

const vcs = new VcsClient({ storePath: STORE_PATH, verbose: argv.verbose });

// Active stack for this agent (one at a time for the demo)
let activeStack = null;

// ── Express app ────────────────────────────────────────────────────────────

const app = express();
app.use(express.json());

function ok(res, data) { res.json({ ok: true, ...data }); }
function err(res, e, status = 500) {
  console.error(`[${AGENT_ID}] ERROR:`, e.message);
  res.status(status).json({ ok: false, error: e.message });
}

// Health
app.get('/health', (_req, res) => {
  ok(res, { agentId: AGENT_ID, storePath: STORE_PATH, activeStack });
});

// Open a stack
app.post('/stack/open', (req, res) => {
  try {
    const { base } = req.body ?? {};
    activeStack = vcs.stackOpen(AGENT_ID, base ?? null);
    ok(res, { stackId: activeStack });
  } catch (e) { err(res, e); }
});

// Close the active stack
app.post('/stack/close', (_req, res) => {
  try {
    if (!activeStack) return err(res, new Error('no active stack'), 400);
    vcs.stackClose(activeStack);
    const closed = activeStack;
    activeStack = null;
    ok(res, { stackId: closed });
  } catch (e) { err(res, e); }
});

// Abandon the active stack
app.post('/stack/abandon', (_req, res) => {
  try {
    if (!activeStack) return err(res, new Error('no active stack'), 400);
    vcs.stackAbandon(activeStack);
    activeStack = null;
    ok(res, {});
  } catch (e) { err(res, e); }
});

// Record an edit
app.post('/edit', (req, res) => {
  try {
    if (!activeStack) return err(res, new Error('no active stack'), 400);
    const { path, content, reason, task_ref, tool_call } = req.body;
    if (!path || content === undefined || !reason) {
      return err(res, new Error('path, content, reason required'), 400);
    }
    const changeId = vcs.edit(activeStack, path, content, { reason, task_ref, tool_call });
    ok(res, { changeId, stackId: activeStack });
  } catch (e) { err(res, e); }
});

// Record a deletion
app.post('/delete', (req, res) => {
  try {
    if (!activeStack) return err(res, new Error('no active stack'), 400);
    const { path, reason, task_ref } = req.body;
    if (!path || !reason) return err(res, new Error('path, reason required'), 400);
    const changeId = vcs.delete(activeStack, path, { reason, task_ref });
    ok(res, { changeId });
  } catch (e) { err(res, e); }
});

// Return the current stack ID
app.get('/stack', (_req, res) => {
  ok(res, { stackId: activeStack, agentId: AGENT_ID });
});

// Return the change log — ?stackId=<id> for a specific stack; defaults to activeStack
app.get('/log', (req, res) => {
  try {
    const stackId = req.query.stackId ?? activeStack;
    if (!stackId) return ok(res, { log: [] });
    const log = vcs.log(stackId);
    ok(res, { log });
  } catch (e) { err(res, e); }
});

// ── Start ──────────────────────────────────────────────────────────────────

app.listen(PORT, () => {
  console.log(`[${AGENT_ID}] listening on :${PORT}  store=${STORE_PATH}`);
});

export default app;
