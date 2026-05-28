export interface CommitFile {
  hash: string
  size: number
}

export interface CommitRecord {
  id: string
  agent: string
  ts: string
  base: string | null
  intent: 'snapshot' | 'promote' | 'checkout'
  reason: string
  files: Record<string, CommitFile>
}

export function storeDir(projectRoot: string): string
export function sha256(content: string | Buffer): string
export function storeBlob(projectRoot: string, content: Buffer | string): string
export function readBlob(projectRoot: string, hash: string): Buffer
export function readLog(projectRoot: string, filter?: { agent?: string }): CommitRecord[]
export function snapshot(
  agentId: string,
  overlayDir: string,
  projectRoot: string,
  intent?: string,
  reason?: string,
): CommitRecord
export function scanDir(dir: string, base?: string, out?: string[]): string[]
