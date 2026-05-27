#!/usr/bin/env node
/**
 * vcs-mcp/server.js — MCP server for vcs-spike.
 *
 * Gives Claude Code (and any MCP-compatible AI) native vcs tools.
 * Claude no longer needs to call the CLI manually — it just uses tools.
 *
 * Usage — add to .mcp.json in your project root:
 *
 *   {
 *     "mcpServers": {
 *       "vcs": {
 *         "command": "node",
 *         "args": ["packages/vcs-mcp/server.js"],
 *         "env": { "VCS_BIN": "./target/release/vcs" }
 *       }
 *     }
 *   }
 *
 * Or globally via Claude Desktop:
 *   ~/.config/claude/claude_desktop_config.json
 *
 * Tools exposed:
 *   vcs_status          — check if store is initialised
 *   vcs_init            — initialise .vcs/ in the current project
 *   vcs_stack_open      — open an agent stack (call at start of every task)
 *   vcs_stack_close     — close when done
 *   vcs_stack_abandon   — abandon on error/cancellation
 *   vcs_edit            — record a file edit (use instead of write_file)
 *   vcs_delete          — record a file deletion
 *   vcs_rename          — record a file rename/move
 *   vcs_view_open       — open a merged view of multiple stacks
 *   vcs_view_files      — list files visible in a view
 *   vcs_view_conflicts  — list conflicts in a view
 *   vcs_resolve         — resolve a conflict
 *   vcs_log             — show change history for a stack
 *   vcs_history         — show full store history across stacks
 *   vcs_checkout        — materialize a change into a worktree
 *   vcs_remote_add      — configure a named remote store
 *   vcs_push            — push this store to a remote hub
 *   vcs_pull            — pull a remote hub bundle into this store
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { spawnSync } from 'node:child_process'
import { writeFileSync, existsSync, rmSync } from 'node:fs'
import { join, resolve, dirname } from 'node:path'
import { tmpdir } from 'node:os'

// ── Binary resolution ──────────────────────────────────────────────────────

function findBin() {
  if (process.env.VCS_BIN && existsSync(process.env.VCS_BIN)) return process.env.VCS_BIN
  const cwd = process.cwd()
  const siblings = [
    join(cwd, 'target/release/vcs'),
    resolve(dirname(new URL(import.meta.url).pathname), '../../target/release/vcs'),
  ]
  for (const p of siblings) if (existsSync(p)) return p
  return 'vcs'
}

const BIN = findBin()

// ── vcs runner ─────────────────────────────────────────────────────────────

function runVcs(args, { cwd, input } = {}) {
  const r = spawnSync(BIN, ['--json', ...args], {
    cwd: cwd ?? process.cwd(),
    input,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (r.status !== 0) throw new Error(r.stderr?.trim() || `vcs ${args[0]} failed`)
  const out = r.stdout?.trim()
  if (!out) return null
  try { return JSON.parse(out) } catch { return { text: out } }
}

function tmpWrite(content) {
  const p = join(tmpdir(), `vcs-mcp-${Date.now()}-${Math.random().toString(36).slice(2)}`)
  writeFileSync(p, typeof content === 'string' ? content : Buffer.from(content))
  return p
}

// ── Tool definitions ───────────────────────────────────────────────────────

const TOOLS = [
  {
    name: 'vcs_status',
    description:
      'Check whether the vcs store is initialised AND whether any other sessions left ' +
      'open stacks. ALWAYS call this at the start of every task. ' +
      'If the response includes open_stacks (non-empty array), another Claude Code session ' +
      'or agent left work in progress — ask the user whether to merge, abandon, or ignore ' +
      'those stacks before starting new work. Never silently ignore open_stacks.',
    inputSchema: {
      type: 'object',
      properties: {
        store_path: {
          type: 'string',
          description: 'Path to .vcs/ directory. Omit to auto-detect (walks up from CWD).',
        },
      },
    },
  },
  {
    name: 'vcs_init',
    description:
      'Initialise a vcs store in the current project (creates .vcs/). ' +
      'Run once per project — like git init. Safe to re-run.',
    inputSchema: {
      type: 'object',
      properties: {
        path: {
          type: 'string',
          description: 'Directory to initialise in. Defaults to current working directory.',
        },
      },
    },
  },
  {
    name: 'vcs_stack_open',
    description:
      'Open a new stack for this agent session. Call this at the START of every task. ' +
      'Returns a stack_id — pass it to every vcs_edit/vcs_delete call.',
    inputSchema: {
      type: 'object',
      required: ['agent_id'],
      properties: {
        agent_id: {
          type: 'string',
          description: 'Unique identifier for this agent (e.g. "claude-code", "refactor-agent").',
        },
        base_change_id: {
          type: 'string',
          description: 'Branch from this change ID. Omit to start from HEAD.',
        },
      },
    },
  },
  {
    name: 'vcs_stack_close',
    description: 'Close the stack when the task is complete. Always call this when done.',
    inputSchema: {
      type: 'object',
      required: ['stack_id'],
      properties: {
        stack_id: { type: 'string', description: 'Stack ID returned by vcs_stack_open.' },
      },
    },
  },
  {
    name: 'vcs_stack_abandon',
    description: 'Abandon a stack on error or cancellation (marks it as dead — cannot be closed).',
    inputSchema: {
      type: 'object',
      required: ['stack_id'],
      properties: {
        stack_id: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_edit',
    description:
      'Record a file edit in vcs. Use this INSTEAD OF write_file when vcs is active. ' +
      'The content is stored in the vcs blob store and linked to your stack. ' +
      'intent.reason is required — explain WHY you are making this change.',
    inputSchema: {
      type: 'object',
      required: ['stack_id', 'path', 'content', 'reason'],
      properties: {
        stack_id: { type: 'string', description: 'Stack to record the edit in.' },
        path:     { type: 'string', description: 'File path (relative to project root).' },
        content:  { type: 'string', description: 'Full new file content.' },
        reason:   { type: 'string', description: 'Why you are making this change (required).' },
        task_ref: { type: 'string', description: 'Optional task/issue reference (e.g. JIRA-123).' },
      },
    },
  },
  {
    name: 'vcs_delete',
    description: 'Record a file deletion in vcs.',
    inputSchema: {
      type: 'object',
      required: ['stack_id', 'path', 'reason'],
      properties: {
        stack_id: { type: 'string' },
        path:     { type: 'string' },
        reason:   { type: 'string' },
        task_ref: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_rename',
    description: 'Record a file rename/move in vcs.',
    inputSchema: {
      type: 'object',
      required: ['stack_id', 'from', 'to', 'content', 'reason'],
      properties: {
        stack_id: { type: 'string' },
        from:     { type: 'string', description: 'Old path.' },
        to:       { type: 'string', description: 'New path.' },
        content:  { type: 'string', description: 'File content at the new path.' },
        reason:   { type: 'string' },
        task_ref: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_view_open',
    description:
      'Open a merged view of one or more stacks. ' +
      'Use this to see the combined file tree and detect conflicts between agents.',
    inputSchema: {
      type: 'object',
      required: ['stack_ids'],
      properties: {
        base_change_id: {
          type: 'string',
          description: 'Base change to merge on top of. Omit or pass "" for root.',
        },
        stack_ids: {
          type: 'array',
          items: { type: 'string' },
          description: 'Stack IDs to merge into the view.',
        },
      },
    },
  },
  {
    name: 'vcs_view_files',
    description: 'List all files visible in a merged view.',
    inputSchema: {
      type: 'object',
      required: ['view_id'],
      properties: {
        view_id: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_view_conflicts',
    description:
      'List all conflicts in a view. A conflict means two stacks edited the same file. ' +
      'Conflicts are data — report them to the user, do not silently resolve.',
    inputSchema: {
      type: 'object',
      required: ['view_id'],
      properties: {
        view_id: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_resolve',
    description:
      'Resolve a conflict by picking one stack or providing merged content. ' +
      'Only call this when explicitly instructed — conflicts are important signals.',
    inputSchema: {
      type: 'object',
      required: ['conflict_id'],
      properties: {
        conflict_id:   { type: 'string' },
        pick_stack_id: {
          type: 'string',
          description: 'Pick the version from this stack.',
        },
        merge_content: {
          type: 'string',
          description: 'Provide hand-merged content (use instead of pick_stack_id).',
        },
      },
    },
  },
  {
    name: 'vcs_log',
    description: 'Show the change history for a stack (newest first).',
    inputSchema: {
      type: 'object',
      required: ['stack_id'],
      properties: {
        stack_id: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_history',
    description: 'Show complete change history across all stacks.',
    inputSchema: {
      type: 'object',
      properties: {
        store_path: { type: 'string', description: 'Optional .vcs store path.' },
      },
    },
  },
  {
    name: 'vcs_checkout',
    description: 'Materialize the tracked file tree at a change ID for replay or testing.',
    inputSchema: {
      type: 'object',
      required: ['change_id'],
      properties: {
        change_id: { type: 'string' },
        worktree: { type: 'string', description: 'Output directory. Defaults to CWD.' },
        store_path: { type: 'string', description: 'Optional .vcs store path.' },
      },
    },
  },
  {
    name: 'vcs_remote_add',
    description: 'Add or update a named remote hub URL for this store.',
    inputSchema: {
      type: 'object',
      required: ['name', 'url'],
      properties: {
        name: { type: 'string' },
        url: { type: 'string' },
        store_path: { type: 'string', description: 'Optional .vcs store path.' },
      },
    },
  },
  {
    name: 'vcs_push',
    description:
      'Push this store to a remote hub as structured agent history, including edit metadata and blobs.',
    inputSchema: {
      type: 'object',
      required: ['remote'],
      properties: {
        remote: { type: 'string', description: 'Named remote or direct http(s) URL.' },
        project_id: { type: 'string', description: 'Project ID to include in the bundle.' },
        store_path: { type: 'string', description: 'Optional .vcs store path.' },
      },
    },
  },
  {
    name: 'vcs_pull',
    description: 'Pull structured agent history from a remote hub into this store.',
    inputSchema: {
      type: 'object',
      required: ['remote'],
      properties: {
        remote: { type: 'string', description: 'Named remote or direct http(s) URL.' },
        store_path: { type: 'string', description: 'Optional .vcs store path.' },
      },
    },
  },
  {
    name: 'vcs_session_open',
    description:
      'Register this Claude Code session with the vcs store. ' +
      'Call ONCE at the very start of every chat session, before vcs_stack_open. ' +
      'Returns a session_id — save it for the whole chat. ' +
      'Pass port if this session will run a dev-server (e.g. 5173) so vcs_overview ' +
      'can show which port each agent is using without port collisions.',
    inputSchema: {
      type: 'object',
      required: ['agent_id'],
      properties: {
        agent_id: {
          type: 'string',
          description: 'Unique ID for this agent session, e.g. "claude-code-feature-auth".',
        },
        port: {
          type: 'number',
          description: 'Dev-server port this session will use (e.g. 5173, 5174). Optional.',
        },
        store_path: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_session_close',
    description: 'Mark this session as done. Call when the task is complete.',
    inputSchema: {
      type: 'object',
      required: ['session_id'],
      properties: {
        session_id: { type: 'string' },
        store_path: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_session_phase',
    description:
      'Set the current session phase: working | testing | done. ' +
      'Call with phase=testing when you are about to run tests or start a dev-server. ' +
      'While any session is in testing phase, other sessions must NOT merge their stacks — ' +
      'vcs_overview will show the gate. Call with phase=done when validation passes.',
    inputSchema: {
      type: 'object',
      required: ['session_id', 'phase'],
      properties: {
        session_id: { type: 'string' },
        phase: {
          type: 'string',
          enum: ['working', 'testing', 'done'],
          description: 'working = still editing | testing = validating, blocks merges | done = ready to merge',
        },
        store_path: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_touching',
    description:
      'Check which other open stacks are currently editing a given file. ' +
      'Call after vcs_edit to get immediate collision warnings — no view needed. ' +
      'Returns other_stacks: [] if you are the only one touching this file.',
    inputSchema: {
      type: 'object',
      required: ['path'],
      properties: {
        path:      { type: 'string', description: 'File path to check.' },
        stack_id:  { type: 'string', description: 'Your stack (excluded from results).' },
        store_path: { type: 'string' },
      },
    },
  },
  {
    name: 'vcs_overview',
    description:
      'Return a complete picture of all agent activity in this project RIGHT NOW: ' +
      'active sessions, what files each agent is touching, and which files will ' +
      'conflict when stacks are merged. ' +
      'This is the primary tool for narrating multi-agent state to the human — ' +
      'no browser required. Call this whenever the user asks "what is happening" ' +
      'or "what are the agents doing".',
    inputSchema: {
      type: 'object',
      properties: {
        store_path: { type: 'string', description: 'Optional .vcs store path.' },
      },
    },
  },
]

// ── Tool handlers ──────────────────────────────────────────────────────────

function handleTool(name, args) {
  const store = args.store_path ? ['--store', args.store_path] : []

  switch (name) {
    case 'vcs_status': {
      try {
        const r = runVcs([...store, 'stack', 'open', '--agent', '__status_check__'])
        if (r?.stack_id) {
          runVcs([...store, 'stack', 'abandon', r.stack_id])
        }
        // Enumerate open stacks so new sessions can detect in-progress work
        // from interrupted sessions and avoid silently ignoring those changes.
        let openStacks = []
        try {
          const ls = runVcs([...store, 'stack', 'ls', '--status', 'open'])
          // Filter out the status-check stack we just abandoned (already gone)
          openStacks = (Array.isArray(ls) ? ls : []).filter(
            s => s.agent_id !== '__status_check__'
          )
        } catch (_) { /* store may be empty — not an error */ }

        const result = { initialised: true, binary: BIN, open_stacks: openStacks }
        if (openStacks.length > 0) {
          result.warning =
            `${openStacks.length} stack(s) from other sessions are still OPEN. ` +
            `Before starting new work, open a view over all open stacks ` +
            `(vcs_view_open) and check for conflicts (vcs_view_conflicts). ` +
            `If a stack belongs to an interrupted session, abandon it with vcs_stack_abandon.`
        }
        return result
      } catch (e) {
        const msg = e.message ?? ''
        if (msg.includes('not initialised') || msg.includes('NotInitialised')) {
          return { initialised: false, binary: BIN, message: 'Run vcs_init first.' }
        }
        return { initialised: true, binary: BIN, open_stacks: [] }
      }
    }

    case 'vcs_init': {
      const initStore = args.path ?? process.cwd()
      return runVcs(['--store', initStore, 'init'])
    }

    case 'vcs_stack_open': {
      const a = ['stack', 'open', '--agent', args.agent_id, ...store]
      if (args.base_change_id) a.push('--base', args.base_change_id)
      const result = runVcs(a)
      // Auto-link the new stack to the session if one was registered
      if (args.session_id && result?.stack_id) {
        try {
          runVcs([...store, 'session', 'link-stack', args.session_id, result.stack_id])
        } catch (_) { /* non-fatal */ }
      }
      return result
    }

    case 'vcs_stack_close':
      return runVcs([...store, 'stack', 'close', args.stack_id])

    case 'vcs_stack_abandon':
      return runVcs([...store, 'stack', 'abandon', args.stack_id])

    case 'vcs_edit': {
      const tmp = tmpWrite(args.content)
      const a = [...store, 'edit', args.stack_id, args.path,
        '--content-file', tmp, '--reason', args.reason]
      if (args.task_ref) a.push('--task-ref', args.task_ref)
      const result = runVcs(a)
      try { rmSync(tmp, { force: true }) } catch {}
      return result
    }

    case 'vcs_delete': {
      const a = [...store, 'delete', args.stack_id, args.path, '--reason', args.reason]
      if (args.task_ref) a.push('--task-ref', args.task_ref)
      return runVcs(a)
    }

    case 'vcs_rename': {
      const tmp = tmpWrite(args.content)
      const a = [...store, 'rename', args.stack_id, args.from, args.to,
        '--content-file', tmp, '--reason', args.reason]
      if (args.task_ref) a.push('--task-ref', args.task_ref)
      const result = runVcs(a)
      try { rmSync(tmp, { force: true }) } catch {}
      return result
    }

    case 'vcs_view_open': {
      const base = args.base_change_id ?? ''
      return runVcs([...store, 'view', 'open',
        '--base', base,
        '--stacks', (args.stack_ids ?? []).join(',')])
    }

    case 'vcs_view_files': {
      const r = runVcs([...store, 'view', 'ls', args.view_id])
      return { files: r?.files ?? [] }
    }

    case 'vcs_view_conflicts':
      return { conflicts: runVcs([...store, 'view', 'conflicts', args.view_id]) ?? [] }

    case 'vcs_resolve': {
      if (args.pick_stack_id) {
        return runVcs([...store, 'view', 'resolve', args.conflict_id, '--pick', args.pick_stack_id])
      } else if (args.merge_content) {
        const tmp = tmpWrite(args.merge_content)
        const result = runVcs([...store, 'view', 'resolve', args.conflict_id, '--merge-file', tmp])
        try { rmSync(tmp, { force: true }) } catch {}
        return result
      }
      throw new Error('Provide either pick_stack_id or merge_content')
    }

    case 'vcs_log':
      return { changes: runVcs([...store, 'log', args.stack_id]) ?? [] }

    case 'vcs_history':
      return { changes: runVcs([...store, 'history']) ?? [] }

    case 'vcs_checkout': {
      const a = [...store, 'checkout', args.change_id]
      if (args.worktree) a.push('--worktree', args.worktree)
      return runVcs(a)
    }

    case 'vcs_remote_add':
      return runVcs([...store, 'remote', 'add', args.name, args.url])

    case 'vcs_push': {
      const a = [...store, 'push', args.remote]
      if (args.project_id) a.push('--project-id', args.project_id)
      return runVcs(a)
    }

    case 'vcs_pull':
      return runVcs([...store, 'pull', args.remote])

    case 'vcs_session_open': {
      const a = [...store, 'session', 'open', '--agent', args.agent_id]
      if (args.port) a.push('--port', String(args.port))
      return runVcs(a)
    }

    case 'vcs_session_close':
      return runVcs([...store, 'session', 'close', args.session_id])

    case 'vcs_session_phase':
      return runVcs([...store, 'session', 'phase', args.session_id, args.phase])

    case 'vcs_touching': {
      const a = [...store, 'touching', args.path]
      if (args.stack_id) a.push('--stack', args.stack_id)
      return runVcs(a)
    }

    case 'vcs_overview':
      return runVcs([...store, 'overview'])

    default:
      throw new Error(`Unknown tool: ${name}`)
  }
}

// ── MCP Server ─────────────────────────────────────────────────────────────

const server = new Server(
  { name: 'vcs-spike', version: '0.1.0' },
  { capabilities: { tools: {} } },
)

server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }))

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params
  try {
    const result = handleTool(name, args ?? {})
    return {
      content: [{
        type: 'text',
        text: JSON.stringify(result, null, 2),
      }],
    }
  } catch (e) {
    return {
      content: [{
        type: 'text',
        text: JSON.stringify({ error: e.message }),
      }],
      isError: true,
    }
  }
})

// ── Start ──────────────────────────────────────────────────────────────────

const transport = new StdioServerTransport()
await server.connect(transport)
