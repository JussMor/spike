import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
// @ts-ignore — JS plugin, types not needed
import { vcsPlugin } from './vcs-integration/vite-plugin.js'

/**
 * The vcsPlugin injects /api/vcs/* endpoints into the Vite dev server.
 * The React app polls these with TanStack Query to show live vcs state.
 *
 * The store lives in .vcs/ in this directory — exactly like .git/ for git.
 * Initialise once with:  npm run vcs:init
 * Then track changes with: npm run vcs:demo  or  npm run vcs:watch
 */
export default defineConfig({
  plugins: [react(), vcsPlugin()],
})
