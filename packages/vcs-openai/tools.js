/**
 * vcs-openai/tools.js — OpenAI function definitions for vcs-spike.
 *
 * Pass these directly to the OpenAI SDK's `tools` parameter.
 * Compatible with Chat Completions API and Assistants API.
 *
 * Usage with Chat Completions:
 *
 *   import { vcsTools, handleVcsTool } from 'vcs-openai'
 *   import OpenAI from 'openai'
 *
 *   const openai = new OpenAI()
 *   const messages = [{ role: 'user', content: 'Add a login component' }]
 *
 *   const response = await openai.chat.completions.create({
 *     model: 'gpt-4o',
 *     messages,
 *     tools: vcsTools,
 *   })
 *
 *   // Handle tool calls in the response
 *   for (const toolCall of response.choices[0].message.tool_calls ?? []) {
 *     const result = await handleVcsTool(
 *       toolCall.function.name,
 *       JSON.parse(toolCall.function.arguments),
 *     )
 *     messages.push({
 *       role: 'tool',
 *       tool_call_id: toolCall.id,
 *       content: JSON.stringify(result),
 *     })
 *   }
 *
 * Usage with Assistants API:
 *
 *   const assistant = await openai.beta.assistants.create({
 *     name: 'vcs-agent',
 *     model: 'gpt-4o',
 *     tools: vcsTools,
 *     instructions: vcsSystemPrompt,
 *   })
 */

import { spawnSync } from 'node:child_process'
import { writeFileSync, existsSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'

// ── Function schemas (OpenAI format) ──────────────────────────────────────

export const vcsTools = [
  {
    type: 'function',
    function: {
      name: 'vcs_status',
      description:
        'Check whether the vcs store is initialised in the current project. ' +
        'Returns { initialised: bool, storePath: string }.',
      parameters: {
        type: 'object',
        properties: {},
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_init',
      description: 'Initialise a vcs store (.vcs/) in the project. Like git init.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Directory to initialise. Defaults to CWD.',
          },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_stack_open',
      description:
        'Open an agent stack. Call at the START of every task. ' +
        'Returns { stack_id } — pass this to all edit calls.',
      parameters: {
        type: 'object',
        required: ['agent_id'],
        properties: {
          agent_id: {
            type: 'string',
            description: 'Unique agent identifier, e.g. "codex-refactor-auth".',
          },
          base_change_id: {
            type: 'string',
            description: 'Branch from this change. Omit for HEAD.',
          },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_stack_close',
      description: 'Close the stack when the task is complete.',
      parameters: {
        type: 'object',
        required: ['stack_id'],
        properties: {
          stack_id: { type: 'string' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_stack_abandon',
      description: 'Abandon a stack on error or cancellation.',
      parameters: {
        type: 'object',
        required: ['stack_id'],
        properties: {
          stack_id: { type: 'string' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_edit',
      description:
        'Record a file edit in vcs. Use this instead of writing files directly. ' +
        'The reason field is required — explain WHY you are making this change.',
      parameters: {
        type: 'object',
        required: ['stack_id', 'path', 'content', 'reason'],
        properties: {
          stack_id: { type: 'string' },
          path:     { type: 'string', description: 'File path relative to project root.' },
          content:  { type: 'string', description: 'Full new file content.' },
          reason:   { type: 'string', description: 'Why you are making this change.' },
          task_ref: { type: 'string', description: 'Optional issue/task reference.' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_delete',
      description: 'Record a file deletion in vcs.',
      parameters: {
        type: 'object',
        required: ['stack_id', 'path', 'reason'],
        properties: {
          stack_id: { type: 'string' },
          path:     { type: 'string' },
          reason:   { type: 'string' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_view_open',
      description:
        'Open a merged view of multiple stacks. ' +
        'Use after all agents finish to detect conflicts.',
      parameters: {
        type: 'object',
        required: ['stack_ids'],
        properties: {
          base_change_id: { type: 'string', description: 'Base. Pass "" for root.' },
          stack_ids: {
            type: 'array',
            items: { type: 'string' },
            description: 'Stack IDs to merge.',
          },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_view_conflicts',
      description:
        'List conflicts in a merged view. ' +
        'Returns array of { path, candidates, resolution }. ' +
        'Report conflicts — do NOT silently pick a winner.',
      parameters: {
        type: 'object',
        required: ['view_id'],
        properties: {
          view_id: { type: 'string' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_view_files',
      description: 'List all files visible in a merged view.',
      parameters: {
        type: 'object',
        required: ['view_id'],
        properties: {
          view_id: { type: 'string' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_resolve',
      description: 'Resolve a conflict. Only call when explicitly told to.',
      parameters: {
        type: 'object',
        required: ['conflict_id'],
        properties: {
          conflict_id:   { type: 'string' },
          pick_stack_id: { type: 'string', description: 'Pick the version from this stack.' },
          merge_content: { type: 'string', description: 'Hand-merged content (alternative to pick).' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_log',
      description: 'Show change history for a stack.',
      parameters: {
        type: 'object',
        required: ['stack_id'],
        properties: {
          stack_id: { type: 'string' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_rename',
      description: 'Record a file rename/move in vcs.',
      parameters: {
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
  },
  {
    type: 'function',
    function: {
      name: 'vcs_history',
      description: 'Show complete change history across all stacks (newest first).',
      parameters: {
        type: 'object',
        properties: {},
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_checkout',
      description:
        'Materialise the tracked file tree at a historical change ID into a directory. ' +
        'Use this to replay or inspect what an agent produced at any point in history.',
      parameters: {
        type: 'object',
        required: ['change_id'],
        properties: {
          change_id: { type: 'string', description: 'Change ID to materialise (from vcs_log or vcs_history).' },
          worktree: {
            type: 'string',
            description: 'Directory to write files into. Defaults to current directory.',
          },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_remote_add',
      description: 'Add or update a named remote hub URL for push/pull operations.',
      parameters: {
        type: 'object',
        required: ['name', 'url'],
        properties: {
          name: { type: 'string', description: 'Remote name (e.g. "hub", "staging").' },
          url:  { type: 'string', description: 'Hub URL (e.g. "http://localhost:7474").' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_push',
      description:
        'Push this store\'s stacks, changes, and blobs to a remote hub. ' +
        'Use after all agents finish to share agent history cross-project.',
      parameters: {
        type: 'object',
        required: ['remote'],
        properties: {
          remote:     { type: 'string', description: 'Named remote or direct http(s) URL.' },
          project_id: { type: 'string', description: 'Project ID to tag the bundle with.' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_pull',
      description:
        'Pull stacks, changes, and blobs from a remote hub into the local store. ' +
        'Idempotent — re-pulling the same bundle is a no-op.',
      parameters: {
        type: 'object',
        required: ['remote'],
        properties: {
          remote: { type: 'string', description: 'Named remote or direct http(s) URL.' },
        },
      },
    },
  },

  // ── Multi-session tools ───────────────────────────────────────────────────

  {
    type: 'function',
    function: {
      name: 'vcs_session_open',
      description:
        'Register this agent as an active session in the vcs store. ' +
        'ALWAYS call this first — before vcs_status or vcs_stack_open. ' +
        'Returns session_id. Save it for the whole chat; pass it to vcs_stack_open.',
      parameters: {
        type: 'object',
        required: ['agent_id'],
        properties: {
          agent_id: { type: 'string', description: 'Unique agent identifier, e.g. "claude-code-feature-auth".' },
          port: { type: 'integer', description: 'Dev-server port this session will use (optional).' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_session_close',
      description:
        'Deregister the session when the task is complete. ' +
        'Call this alongside vcs_stack_close when done, or on cancellation/error.',
      parameters: {
        type: 'object',
        required: ['session_id'],
        properties: {
          session_id: { type: 'string', description: 'Session ID returned by vcs_session_open.' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_session_phase',
      description:
        'Advance the session phase. ' +
        'working → testing (dev-server is live; other sessions must not merge). ' +
        'testing → done (gate lifts; other sessions may now merge). ' +
        'The vcs-vite plugin manages this automatically — only call manually if not using the plugin.',
      parameters: {
        type: 'object',
        required: ['session_id', 'phase'],
        properties: {
          session_id: { type: 'string', description: 'Session ID.' },
          phase: {
            type: 'string',
            enum: ['working', 'testing', 'done'],
            description: 'New phase for the session.',
          },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_touching',
      description:
        'Check whether other open stacks are also editing a specific file. ' +
        'Call this after EVERY vcs_edit. ' +
        'If other_stacks is non-empty, warn the user immediately: ' +
        '"⚡ <agent> is also editing <path> — conflict likely on merge".',
      parameters: {
        type: 'object',
        required: ['path'],
        properties: {
          path: { type: 'string', description: 'Project-relative file path, e.g. "src/auth.ts".' },
          stack_id: { type: 'string', description: 'Your current stack ID (to exclude from results).' },
        },
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'vcs_overview',
      description:
        'Return a full multi-agent picture of the store: all sessions, all open stacks, ' +
        'hot files (files touched by multiple stacks), and a human-readable summary. ' +
        'Call this when vcs_status shows open_stacks, or whenever you need to understand ' +
        'what other agents are doing. The summary field can be shown directly to the user.',
      parameters: {
        type: 'object',
        properties: {},
      },
    },
  },
]

// ── System prompt for OpenAI agents ──────────────────────────────────────

export const vcsSystemPrompt = `You are a coding agent with access to vcs-spike for structured change tracking.

## Rules — multi-session aware

0. ALWAYS register yourself first: vcs_session_open({ agent_id: "<your-id>" }) → save session_id
1. Check the store: vcs_status() — if open_stacks is non-empty, call vcs_overview() and report
2. Open a stack before any edits: vcs_stack_open({ agent_id, session_id })
3. Use vcs_edit instead of writing files directly — it records your intent
4. reason is REQUIRED on every vcs_edit. Be precise about why, not what.
5. After EVERY vcs_edit call vcs_touching({ path, stack_id }) — if other_stacks is non-empty,
   warn the user: "⚡ <agent> is also editing <path> — conflict likely on merge"
6. Close everything when done: vcs_stack_close({ stack_id }) + vcs_session_close({ session_id })
7. On error or cancellation: vcs_stack_abandon({ stack_id }) + vcs_session_close({ session_id })
8. NEVER resolve conflicts without explicit user instruction. Report them.

## Conflict protocol

If vcs_view_conflicts returns conflicts:
- List each conflict clearly (path + which stacks disagree)
- Stop and ask the user which version to keep
- Only call vcs_resolve when the user explicitly says so

## vcs_overview — when to use

Call vcs_overview() whenever:
- vcs_status() shows open_stacks is non-empty
- You want to understand the full picture before merging
- The user asks "what are the other agents doing?"

The summary field in the response is human-readable — show it directly to the user.

## intent.reason examples

Good:  "add login form with email/password validation"
Good:  "fix null check in useAuth — resolves crash on logout"
Bad:   "update file"
Bad:   "make changes as requested"
`

// ── Tool handler (executes against vcs binary) ────────────────────────────

function findBin() {
  if (process.env.VCS_BIN && existsSync(process.env.VCS_BIN)) return process.env.VCS_BIN
  const ws = join(process.cwd(), 'target/release/vcs')
  if (existsSync(ws)) return ws
  return 'vcs'
}

const BIN = findBin()

function run(args, input) {
  const r = spawnSync(BIN, ['--json', ...args], {
    cwd: process.cwd(),
    input,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (r.status !== 0) throw new Error(r.stderr?.trim() || `vcs ${args[0]} failed`)
  const out = r.stdout?.trim()
  if (!out) return { ok: true }
  try { return JSON.parse(out) } catch { return { text: out } }
}

function tmp(content) {
  const p = join(tmpdir(), `vcs-oai-${Date.now()}-${Math.random().toString(36).slice(2)}`)
  writeFileSync(p, content, 'utf8')
  return p
}

/**
 * Execute a vcs tool call.
 * Drop this into your OpenAI tool-call dispatch loop.
 *
 * @param {string} name   Function name from vcsTools
 * @param {object} args   Parsed function arguments
 * @returns {object}      Result to pass back as tool message content
 */
export async function handleVcsTool(name, args = {}) {
  switch (name) {
    case 'vcs_status': {
      try {
        run(['stack', 'open', '--agent', '__ping__'])
        run(['stack', 'open', '--agent', '__ping__'])
      } catch {}
      return { initialised: true, binary: BIN }
    }
    case 'vcs_init':
      return run(['--store', args.path ?? process.cwd(), 'init'])
    case 'vcs_stack_open': {
      const a = ['stack', 'open', '--agent', args.agent_id]
      if (args.base_change_id) a.push('--base', args.base_change_id)
      return run(a)
    }
    case 'vcs_stack_close':
      return run(['stack', 'close', args.stack_id])
    case 'vcs_stack_abandon':
      return run(['stack', 'abandon', args.stack_id])
    case 'vcs_edit': {
      const t = tmp(args.content); try {
        const a = ['edit', args.stack_id, args.path, '--content-file', t, '--reason', args.reason]
        if (args.task_ref) a.push('--task-ref', args.task_ref)
        return run(a)
      } finally { rmSync(t, { force: true }) }
    }
    case 'vcs_delete': {
      const a = ['delete', args.stack_id, args.path, '--reason', args.reason]
      return run(a)
    }
    case 'vcs_view_open':
      return run(['view', 'open', '--base', args.base_change_id ?? '', '--stacks', (args.stack_ids ?? []).join(',')])
    case 'vcs_view_conflicts':
      return { conflicts: run(['view', 'conflicts', args.view_id]) ?? [] }
    case 'vcs_view_files':
      return { files: run(['view', 'ls', args.view_id])?.files ?? [] }
    case 'vcs_resolve': {
      if (args.pick_stack_id) return run(['view', 'resolve', args.conflict_id, '--pick', args.pick_stack_id])
      if (args.merge_content) {
        const t = tmp(args.merge_content); try {
          return run(['view', 'resolve', args.conflict_id, '--merge-file', t])
        } finally { rmSync(t, { force: true }) }
      }
      throw new Error('Provide pick_stack_id or merge_content')
    }
    case 'vcs_log':
      return { changes: run(['log', args.stack_id]) ?? [] }
    case 'vcs_rename': {
      const t = tmp(args.content); try {
        const a = ['rename', args.stack_id, args.from, args.to,
          '--content-file', t, '--reason', args.reason]
        if (args.task_ref) a.push('--task-ref', args.task_ref)
        return run(a)
      } finally { rmSync(t, { force: true }) }
    }
    case 'vcs_history':
      return { changes: run(['history']) ?? [] }
    case 'vcs_checkout': {
      const a = ['checkout', args.change_id]
      if (args.worktree) a.push('--worktree', args.worktree)
      return run(a)
    }
    case 'vcs_remote_add':
      return run(['remote', 'add', args.name, args.url])
    case 'vcs_push': {
      const a = ['push', args.remote]
      if (args.project_id) a.push('--project-id', args.project_id)
      return run(a)
    }
    case 'vcs_pull':
      return run(['pull', args.remote])

    // ── Multi-session ─────────────────────────────────────────────────────
    case 'vcs_session_open': {
      const a = ['session', 'open', '--agent', args.agent_id]
      if (args.port) a.push('--port', String(args.port))
      return run(a)
    }
    case 'vcs_session_close':
      return run(['session', 'close', args.session_id])
    case 'vcs_session_phase':
      return run(['session', 'phase', args.session_id, args.phase])
    case 'vcs_touching': {
      const a = ['touching', args.path]
      if (args.stack_id) a.push('--stack', args.stack_id)
      return run(a)
    }
    case 'vcs_overview':
      return run(['overview'])

    default:
      throw new Error(`Unknown vcs tool: ${name}`)
  }
}
