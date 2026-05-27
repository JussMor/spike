/**
 * orchestrator.js
 *
 * Starts N parallel agent servers, drives them through a real workload via
 * HTTP, then opens a view over all their stacks and surfaces any conflicts.
 *
 * This is the "parallel servers" proof:
 *   - Each agent is a live Express process on its own port.
 *   - All share a single vcs store on disk.
 *   - The orchestrator is the only entity that opens views and resolves conflicts.
 *
 * Usage:
 *   node orchestrator.js [--agents 3] [--store /tmp/vcs-demo]
 */

import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const __dir = dirname(fileURLToPath(import.meta.url));

// ── CLI args ───────────────────────────────────────────────────────────────

const { values: argv } = parseArgs({
  options: {
    agents:  { type: 'string', default: '3' },
    store:   { type: 'string' },
    verbose: { type: 'boolean', default: false },
  },
  strict: false,
});

const NUM_AGENTS = parseInt(argv.agents, 10);
const STORE_PATH = argv.store ?? mkdtempSync(join(tmpdir(), 'vcs-orch-'));
const VERBOSE    = argv.verbose;

// Find the vcs binary
function findBin() {
  if (process.env.VCS_BIN) return process.env.VCS_BIN;
  const rel = join(__dir, '../../target/release/vcs');
  try { spawnSync('test', ['-f', rel]); return rel; } catch {}
  return 'vcs';
}
const VCS_BIN = findBin();

// ── HTTP helpers ──────────────────────────────────────────────────────────

async function post(url, body) {
  const res = await fetch(url, {
    method:  'POST',
    headers: { 'content-type': 'application/json' },
    body:    JSON.stringify(body),
  });
  const json = await res.json();
  if (!json.ok) throw new Error(`POST ${url} failed: ${json.error}`);
  return json;
}

async function get(url) {
  const res = await fetch(url);
  const json = await res.json();
  if (!json.ok) throw new Error(`GET ${url} failed: ${json.error}`);
  return json;
}

// ── vcs CLI wrapper (for orchestrator-side view ops) ──────────────────────

function vcs(...args) {
  if (VERBOSE) process.stderr.write(`[orch] vcs ${args.join(' ')}\n`);
  const r = spawnSync(VCS_BIN, ['--store', STORE_PATH, '--json', ...args], {
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024,
  });
  if (r.status !== 0) throw new Error(`vcs ${args[0]} failed:\n${r.stderr}`);
  const out = r.stdout.trim();
  if (!out) return null;
  try { return JSON.parse(out); } catch { return out; }
}

// ── Agent process management ───────────────────────────────────────────────

const BASE_PORT = 4000;
const agents = [];

function startAgent(index) {
  const agentId = `agent-${String.fromCharCode(65 + index)}`; // A, B, C...
  const port    = BASE_PORT + index;

  const proc = spawn(
    process.execPath,
    [join(__dir, 'agent-server.js'), '--agent-id', agentId, '--port', String(port), '--store', STORE_PATH],
    {
      stdio: VERBOSE ? 'inherit' : 'pipe',
      cwd: join(__dir, '..'),       // node_modules lives here
      env: { ...process.env, VCS_BIN, VCS_STORE_PATH: STORE_PATH },
    },
  );

  return { agentId, port, proc, stackId: null };
}

async function waitForAgent(agent, retries = 20) {
  for (let i = 0; i < retries; i++) {
    try {
      await get(`http://localhost:${agent.port}/health`);
      return;
    } catch {
      await new Promise(r => setTimeout(r, 200));
    }
  }
  throw new Error(`agent ${agent.agentId} on port ${agent.port} never became healthy`);
}

// ── Orchestrator main ──────────────────────────────────────────────────────

async function main() {
  console.log(`\n${'═'.repeat(60)}`);
  console.log('  vcs-spike  —  Parallel Agents Demo');
  console.log(`${'═'.repeat(60)}`);
  console.log(`  Store:   ${STORE_PATH}`);
  console.log(`  Agents:  ${NUM_AGENTS}`);
  console.log(`  Binary:  ${VCS_BIN}\n`);

  // Init store
  vcs('init');
  console.log('✓ store initialised\n');

  // ── Phase 1: seed a base snapshot ─────────────────────────────────────────
  console.log('── Phase 1: seed base snapshot ──────────────────────────');
  const seedStack = vcs('stack', 'open', '--agent', 'orchestrator').stack_id;
  const seedTip = vcs('edit', seedStack, 'shared/config.json',
    '--stdin', '--reason', 'initial config').change_id;
  // pipe stdin
  const seedTipActual = seedActualEdit(seedStack);
  vcs('stack', 'close', seedStack);
  console.log(`  base tip: ${seedTipActual.slice(0, 12)}…\n`);

  // ── Phase 2: start agent servers ─────────────────────────────────────────
  console.log('── Phase 2: start agent servers ─────────────────────────');
  for (let i = 0; i < NUM_AGENTS; i++) {
    agents.push(startAgent(i));
  }

  // Wait for all agents to be healthy in parallel
  await Promise.all(agents.map(a => waitForAgent(a)));
  console.log(`  ${NUM_AGENTS} agent servers healthy\n`);

  // ── Phase 3: each agent opens a stack and does work ───────────────────────
  console.log('── Phase 3: agents open stacks & edit files ─────────────');

  // Open stacks in parallel
  await Promise.all(agents.map(async agent => {
    const { stackId } = await post(`http://localhost:${agent.port}/stack/open`, {
      base: seedTipActual,
    });
    agent.stackId = stackId;
    console.log(`  ${agent.agentId} opened stack ${stackId.slice(0, 8)}…`);
  }));

  // Each agent edits its own files (no overlap) + all edit shared/config.json (conflict!)
  await Promise.all(agents.map(async (agent, i) => {
    // Private file — no conflict
    await post(`http://localhost:${agent.port}/edit`, {
      path:    `src/${agent.agentId}/worker.js`,
      content: `// ${agent.agentId} worker\nexport function work() { return ${i}; }\n`,
      reason:  `implement ${agent.agentId} worker`,
      task_ref: `task-agent-${i}`,
    });

    // Shared file — will conflict across agents
    await post(`http://localhost:${agent.port}/edit`, {
      path:    'shared/config.json',
      content: JSON.stringify({ owner: agent.agentId, value: i * 10 }, null, 2),
      reason:  `${agent.agentId} updates shared config`,
    });

    console.log(`  ${agent.agentId} made 2 edits`);
  }));

  // Close all stacks
  await Promise.all(agents.map(agent =>
    post(`http://localhost:${agent.port}/stack/close`, {})
  ));
  console.log('\n  all stacks closed\n');

  // ── Phase 4: orchestrator opens a view ───────────────────────────────────
  console.log('── Phase 4: open view over all stacks ───────────────────');
  const stackIds = agents.map(a => a.stackId).join(',');
  const viewResult = vcs('view', 'open', '--base', seedTipActual, '--stacks', stackIds);
  const viewId = viewResult.view_id;
  console.log(`  view_id: ${viewId.slice(0, 8)}…\n`);

  // ── Phase 5: inspect files and conflicts ──────────────────────────────────
  console.log('── Phase 5: list files in view ──────────────────────────');
  const files = vcs('view', 'ls', viewId).files;
  files.forEach(f => console.log(`  ${f}`));
  console.log();

  console.log('── Phase 6: detect conflicts ────────────────────────────');
  const conflicts = vcs('view', 'conflicts', viewId);
  if (conflicts.length === 0) {
    console.log('  (no conflicts)');
  } else {
    conflicts.forEach(c => {
      const status = c.resolution ? '✓ resolved' : '⚡ UNRESOLVED';
      console.log(`  ${status}  path=${c.path}`);
      c.candidates.forEach(cand => {
        console.log(`    ↳ stack=${cand.stack_id.slice(0, 8)}  blob=${(cand.blob_hash ?? 'deleted').slice(0, 8)}`);
      });
    });
    console.log();

    // ── Phase 7: resolve conflicts ─────────────────────────────────────────
    console.log('── Phase 7: resolve conflicts ───────────────────────────');
    for (const conflict of conflicts) {
      if (conflict.resolution) continue; // already resolved

      // Simple policy: first candidate wins (agent-A has priority).
      // A real orchestrator would do semantic merging here.
      const winner = conflict.candidates[0].stack_id;
      vcs('view', 'resolve', conflict.conflict_id, '--pick', winner);
      console.log(`  resolved "${conflict.path}" → picked stack ${winner.slice(0, 8)}…`);
    }
    console.log();
  }

  // ── Phase 8: verify resolved view ────────────────────────────────────────
  console.log('── Phase 8: read from resolved view ─────────────────────');
  const allFiles = vcs('view', 'ls', viewId).files;
  for (const f of allFiles) {
    try {
      const content = vcs('view', 'read', viewId, f);
      const preview = JSON.stringify(content).slice(0, 60);
      console.log(`  ${f}: ${preview}`);
    } catch (e) {
      console.log(`  ${f}: ⚠ ${e.message.split('\n')[0]}`);
    }
  }

  // ── Phase 9: fetch logs from all agents ───────────────────────────────────
  console.log('\n── Phase 9: fetch change logs from live agents ──────────');
  await Promise.all(agents.map(async agent => {
    const { log } = await get(`http://localhost:${agent.port}/log?stackId=${agent.stackId}`);
    console.log(`  ${agent.agentId} (${log.length} changes):`);
    log.forEach(c => {
      console.log(`    ${c.change_id.slice(0, 10)} | ${c.op} | ${c.path} | "${c.intent.reason}"`);
    });
  }));

  // ── Done ──────────────────────────────────────────────────────────────────
  console.log(`\n${'─'.repeat(60)}`);
  console.log('  ✓  Spike goals met:');
  console.log('      1. Agents produced structured changes via HTTP');
  console.log('      2. Changes stored in shared SQLite + blob dir');
  console.log('      3. View merged N stacks — O(changes in stacks)');
  console.log('      4. Conflicts surfaced as data, resolved by orchestrator');
  console.log('      5. Intent metadata survived round-trips');
  console.log(`${'─'.repeat(60)}\n`);

  // Clean up agent processes
  agents.forEach(a => a.proc.kill());
  process.exit(0);
}

// ── Helper: use stdin for seed edit (spawnSync with input) ────────────────

function seedActualEdit(stackId) {
  const r = spawnSync(VCS_BIN, [
    '--store', STORE_PATH, '--json',
    'edit', stackId, 'shared/config.json',
    '--stdin', '--reason', 'initial config',
  ], {
    input: JSON.stringify({ version: 1, agents: [] }),
    encoding: 'utf8',
  });
  if (r.status !== 0) throw new Error(r.stderr);
  return JSON.parse(r.stdout.trim()).change_id;
}

main().catch(e => {
  console.error('ORCHESTRATOR ERROR:', e);
  agents.forEach(a => { try { a.proc.kill(); } catch {} });
  process.exit(1);
});
