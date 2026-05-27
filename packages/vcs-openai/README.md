# vcs-openai — OpenAI Codex / GPT-4o integration

Ready-made OpenAI function definitions and plugin manifest for vcs-spike.

## Quick start

```js
import OpenAI from 'openai'
import { vcsTools, vcsSystemPrompt, handleVcsTool } from 'vcs-openai'

const openai = new OpenAI()

// 1. Pass vcsTools + vcsSystemPrompt to your chat completion
const response = await openai.chat.completions.create({
  model: 'gpt-4o',
  messages: [
    { role: 'system', content: vcsSystemPrompt },
    { role: 'user',   content: 'Add a LoginForm component to src/components/' },
  ],
  tools: vcsTools,
})

// 2. Dispatch tool calls
for (const call of response.choices[0].message.tool_calls ?? []) {
  const result = await handleVcsTool(
    call.function.name,
    JSON.parse(call.function.arguments),
  )
  // push result back as { role: 'tool', ... }
}
```

## What's included

### `tools.js` — function definitions

```js
import { vcsTools }        from 'vcs-openai'  // OpenAI tools array
import { vcsSystemPrompt } from 'vcs-openai'  // system prompt with rules
import { handleVcsTool }   from 'vcs-openai'  // tool call dispatcher
```

### `openapi.yaml` — full API spec

Complete OpenAPI 3.1 spec for the `vcs serve` hub API.
Serve this from your hub to make it auto-discoverable:

```bash
vcs serve --port 7474
# Then: GET http://localhost:7474/openapi.yaml
```

### `ai-plugin.json` — ChatGPT plugin manifest

Points to the hub's OpenAPI spec. Useful for custom GPT actions.

## Tools

| Function | Description |
|---|---|
| `vcs_status` | Check store health |
| `vcs_init` | Initialise `.vcs/` |
| `vcs_stack_open` | Start tracking a task |
| `vcs_stack_close` | Finish a task |
| `vcs_stack_abandon` | Cancel a task |
| `vcs_edit` | Record a file edit |
| `vcs_delete` | Record a file deletion |
| `vcs_view_open` | Merge stacks into a view |
| `vcs_view_conflicts` | Check for conflicts |
| `vcs_view_files` | List files in view |
| `vcs_resolve` | Resolve a conflict |
| `vcs_log` | Show change history |

## Conflict protocol

The system prompt instructs the model:
- **Report** conflicts — never silently pick a winner
- **Stop** and ask the user which version to keep
- **Only resolve** when the user explicitly says so
