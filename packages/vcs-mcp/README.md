# vcs-mcp — Claude Code MCP server

Gives Claude Code native vcs tools. Once connected, Claude automatically
tracks every file it edits through vcs — no manual CLI calls needed.

## Setup

### 1. Add to your project's `.mcp.json`

```json
{
  "mcpServers": {
    "vcs": {
      "command": "node",
      "args": ["packages/vcs-mcp/server.js"],
      "env": {
        "VCS_BIN": "./target/release/vcs"
      }
    }
  }
}
```

Claude Code auto-loads `.mcp.json` from the project root.

### 2. Or install globally (Claude Desktop)

Add to `~/.config/claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "vcs": {
      "command": "npx",
      "args": ["vcs-mcp"],
      "env": {}
    }
  }
}
```

## Tools available to Claude

| Tool | What it does |
|---|---|
| `vcs_status` | Check if store is initialised |
| `vcs_init` | Initialise `.vcs/` in the project |
| `vcs_stack_open` | Open an agent stack (start of every task) |
| `vcs_stack_close` | Close when done |
| `vcs_stack_abandon` | Abandon on error |
| `vcs_edit` | Record a file edit (replaces write_file) |
| `vcs_delete` | Record a file deletion |
| `vcs_rename` | Record a file rename/move |
| `vcs_view_open` | Merge multiple stacks into a view |
| `vcs_view_files` | List files in a merged view |
| `vcs_view_conflicts` | Detect conflicts between agents |
| `vcs_resolve` | Resolve a conflict |
| `vcs_log` | Show change history for a stack |

## What changes

Once the MCP server is connected, Claude follows this workflow automatically
(as instructed by `CLAUDE.md`):

```
Before:  Claude writes files directly → no history, no conflict detection
After:   Claude uses vcs_edit → full history, conflict-aware, intent-documented
```

## Custom slash commands

This project includes `.claude/commands/` with:

- `/vcs-start <task-description>` — open a stack, begin tracking
- `/vcs-done` — close stack, show summary
- `/vcs-abort` — abandon stack on error
- `/vcs-status` — show what's been tracked
