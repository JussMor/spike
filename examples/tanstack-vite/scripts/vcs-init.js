/**
 * vcs-init.js
 *
 * Run once to initialise vcs in this project — like `git init`.
 * Creates .vcs/ in the project root (auto-detected by the vcs binary).
 *
 *   npm run vcs:init
 */

import { vcs, PROJECT_ROOT } from '../vcs-integration/client.js'
import { existsSync } from 'node:fs'
import { join } from 'node:path'

const store = join(PROJECT_ROOT, '.vcs')

if (existsSync(join(store, 'vcs.db'))) {
  console.log(`✓ Already initialised at ${store}`)
  console.log('  Run npm run vcs:demo to track this project\'s files')
} else {
  const result = vcs.init()
  console.log(`✓ Initialised vcs store at ${result.path}`)
  console.log()
  console.log('  This is your .vcs/ directory — same idea as .git/.')
  console.log('  The vcs binary auto-detects it when you run any vcs command')
  console.log('  from inside this project.')
  console.log()
  console.log('  Next steps:')
  console.log('    npm run vcs:demo   — prove tracking works')
  console.log('    npm run vcs:agents — parallel agents demo')
  console.log('    npm run dev        — start dev server, see live vcs state')
}
