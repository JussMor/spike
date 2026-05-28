import type { Plugin } from 'vite'

export interface SessionOverlayOptions {
  sessionId: string
  overlayDir: string
}

export function sessionOverlay(options: SessionOverlayOptions): Plugin
