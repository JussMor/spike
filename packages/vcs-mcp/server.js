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
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { spawnSync } from 'node:child_process'
import { writeFileSync, existsSync } from 'node:fs'
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
      'Check whether the vcs store is initialised in the current project. ' +
      'Call this first to know if you need to run vcs_init.',
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
        return { initialised: true, binary: BIN }
      } catch (e) {
        const msg = e.message ?? ''
        if (msg.includes('not initialised') || msg.includes('NotInitialised')) {
          return { initialised: false, binary: BIN, message: 'Run vcs_init first.' }
        }
        return { initialised: true, binary: BIN }
      }
    }

    case 'vcs_init': {
      const initStore = args.path ?? process.cwd()
      return runVcs(['--store', initStore, 'init'])
    }

    case 'vcs_stack_open': {
      const a = ['stack', 'open', '--agent', args.agent_id, ...store]
      if (args.base_change_id) a.push('--base', args.base_change_id)
      return runVcs(a)
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
      try { require('node:fs').rmSync(tmp, { force: true }) } catch {}
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
      try { require('node:fs').rmSync(tmp, { force: true }) } catch {}
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
        try { require('node:fs').rmSync(tmp, { force: true }) } catch {}
        return result
      }
      throw new Error('Provide either pick_stack_id or merge_content')
    }

    case 'vcs_log':
      return { changes: runVcs([...store, 'log', args.stack_id]) ?? [] }

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
