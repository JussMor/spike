export interface SessionOverlayOptions {
  sessionId: string
  overlayDir: string
}

// Structural type accepted by Vite's plugins array without importing
// from a specific vite version — avoids duplicate-types issues in workspaces.
export interface VitePlugin {
  name: string
  enforce?: 'pre' | 'post'
  [key: string]: unknown
}

export function sessionOverlay(options: SessionOverlayOptions): VitePlugin
