# tanstack-vite — vcs-spike integration example

A real Vite + React + TanStack Query project whose source files are tracked
by **vcs-spike**, the agent-native VCS from this monorepo.

This is the proof that the spike works on an actual codebase, not just toy
data.  The `vcs` binary auto-detects the `.vcs/` store by walking up from CWD
— **exactly like `git` finds `.git/`**.

## Quick start

```bash
# 1. Build the vcs binary (once, from repo root)
cargo build --release

# 2. Install JS deps
npm install

# 3. Init vcs in this project — like `git init`
npm run vcs:init

# 4. Prove tracking works
npm run vcs:demo

# 5. Parallel agents stress test
npm run vcs:agents 4

# 6. Start the dev server — TanStack Query shows live vcs state
npm run dev
# → http://localhost:5173
```

## How it's wired

The `vcs` binary auto-detects `.vcs/` by walking up from CWD (same as git
finds `.git/`).  `vcs-integration/vite-plugin.js` injects `/api/vcs/*`
endpoints into the Vite dev server.  The React components use TanStack Query
hooks (`src/hooks/useVcs.ts`) to poll those endpoints every 3 seconds.

```
.vcs/                   ← the store (like .git/)
vcs-integration/        ← Node.js client + Vite plugin
scripts/                ← vcs:init / vcs:demo / vcs:agents
src/hooks/useVcs.ts     ← TanStack Query hooks
src/components/         ← FileTree, ChangeLog, ConflictPanel
```

## What was proven

| Claim | Result |
|---|---|
| vcs auto-detects .vcs/ like git finds .git/ | ✓ |
| Real TSX/CSS/JSON source files tracked as blobs | ✓ |
| Feature agent adds new files — clean merge | ✓ |
| Two agents edit same file → conflict surfaced as data | ✓ |
| Orchestrator merges content, view readable | ✓ |
| 4 concurrent workers write to SQLite, zero data loss | ✓ |
| TanStack Query queries live vcs state via Vite plugin | ✓ |

Currently, two official plugins are available:

- [@vitejs/plugin-react](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react) uses [Oxc](https://oxc.rs)
- [@vitejs/plugin-react-swc](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react-swc) uses [SWC](https://swc.rs/)

## React Compiler

The React Compiler is not enabled on this template because of its impact on dev & build performances. To add it, see [this documentation](https://react.dev/learn/react-compiler/installation).

## Expanding the ESLint configuration

If you are developing a production application, we recommend updating the configuration to enable type-aware lint rules:

```js
export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      // Other configs...

      // Remove tseslint.configs.recommended and replace with this
      tseslint.configs.recommendedTypeChecked,
      // Alternatively, use this for stricter rules
      tseslint.configs.strictTypeChecked,
      // Optionally, add this for stylistic rules
      tseslint.configs.stylisticTypeChecked,

      // Other configs...
    ],
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.node.json', './tsconfig.app.json'],
        tsconfigRootDir: import.meta.dirname,
      },
      // other options...
    },
  },
])
```

You can also install [eslint-plugin-react-x](https://github.com/Rel1cx/eslint-react/tree/main/packages/plugins/eslint-plugin-react-x) and [eslint-plugin-react-dom](https://github.com/Rel1cx/eslint-react/tree/main/packages/plugins/eslint-plugin-react-dom) for React-specific lint rules:

```js
// eslint.config.js
import reactX from 'eslint-plugin-react-x'
import reactDom from 'eslint-plugin-react-dom'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      // Other configs...
      // Enable lint rules for React
      reactX.configs['recommended-typescript'],
      // Enable lint rules for React DOM
      reactDom.configs.recommended,
    ],
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.node.json', './tsconfig.app.json'],
        tsconfigRootDir: import.meta.dirname,
      },
      // other options...
    },
  },
])
```
