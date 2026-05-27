/**
 * vcs-spike — main entry point.
 *
 * @example Local project (single store, git-like):
 *   import { createLocalClient } from 'vcs-spike'
 *   const vcs = createLocalClient()        // auto-detects .vcs/ in CWD
 *   const stackId = vcs.stackOpen('my-agent')
 *   vcs.edit(stackId, 'src/foo.ts', code, { reason: 'add login' })
 *
 * @example Multi-project hub (connect frontend + backend):
 *   import { VcsRemoteClient } from 'vcs-spike/remote'
 *   const hub = new VcsRemoteClient('http://localhost:7474')
 *   const stackId = await hub.stackOpen('agent-frontend')
 *   await hub.edit(stackId, 'src/api.ts', code, { reason: 'define endpoint' })
 */

export { VcsRemoteClient } from './remote-client.js'
export { buildBundle }     from './hub-client.js'

// Re-export a factory for the local CLI-based client
// (same API shape as VcsRemoteClient but synchronous + spawns binary)
export function createLocalClient(projectRoot) {
  // Dynamic import so the binary resolution only runs when needed
  return import('./client.js').then(m => m.createClient(projectRoot))
}
