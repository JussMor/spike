# vcs-dev-server

Framework-neutral dev-server runtime for **vcs-spike** agent workflows.

`vcs-vite` is the deep Vite integration. This package is the shared substrate
for other stacks: one session, one stack, one server, all source reads coming
from the vcs store instead of from shared disk.

## What It Provides

| Layer | Use case |
|---|---|
| `createVcsAgentRuntime()` | Framework adapters: Next/Webpack/Rollup/custom compilers can read files from a stack view and poll precise changes. |
| `vcsSourceMiddleware()` | Express/Connect/custom Node servers can serve tracked files from the store before falling through to disk. |
| `vcs-dev-server` CLI | Static HTML/JS/CSS projects can run a dedicated per-stack server immediately. |

## Static Server

```bash
vcs-dev-server \
  --root . \
  --store .vcs \
  --stack "$VCS_STACK_ID" \
  --session "$VCS_SESSION_ID" \
  --port 8787
```

`--dir` is accepted as an alias for `--root` for compatibility with common
static-server prompts:

```bash
vcs-dev-server --dir . --store .vcs --stack "$VCS_STACK_ID" --port 8787
```

Endpoints:

```text
GET /api/vcs-agent/session  # current session/stack/view/tracked files
GET /api/vcs-agent/events   # server-sent change events
GET /src/app.js             # served from the stack view if tracked, otherwise disk
```

## Middleware

```js
import express from 'express'
import { vcsSourceMiddleware } from 'vcs-dev-server'

const app = express()

app.use(vcsSourceMiddleware({
  root: process.cwd(),
  storePath: '.vcs',
  stackId: process.env.VCS_STACK_ID,
  sessionId: process.env.VCS_SESSION_ID,
}))

app.use(express.static('.'))
app.listen(8787)
```

## Adapter Runtime

```js
import { createVcsAgentRuntime } from 'vcs-dev-server'

const vcs = createVcsAgentRuntime({
  root: process.cwd(),
  storePath: '.vcs',
  stackId: process.env.VCS_STACK_ID,
  sessionId: process.env.VCS_SESSION_ID,
})

vcs.start()

const content = vcs.read('src/app.ts')
vcs.onChange(({ changed, deleted }) => {
  // invalidate this framework's module graph here
})
```

## Stack Guidance

Use the lowest adapter layer that can intercept the framework's source reads:

| Stack | Integration |
|---|---|
| Vite, Vue, SvelteKit, Astro, Nuxt Vite mode | Use `vcs-vite` because those stacks expose Vite module hooks. |
| Static HTML/JS/CSS | Use `vcs-dev-server` CLI. |
| Express, Connect, custom Node servers | Use `vcsSourceMiddleware()`. |
| Next.js/Webpack/Turbopack | Needs a compiler adapter that uses `createVcsAgentRuntime()` to overlay module reads and invalidate the graph. Do not create worktrees. |
| Python/Rails/Django/PHP | Serve generated/static files through the middleware pattern or add a file-loader adapter for that runtime. Do not share one mutable source tree between agents. |

The invariant stays the same across stacks:

```text
Store is truth. Disk is fallback. Checkout is export only.
```
