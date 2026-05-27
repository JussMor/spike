/**
 * useVcs.ts — TanStack Query hooks for the vcs dev-server API.
 *
 * The Vite dev server runs a companion express middleware (see vite.config.ts)
 * that exposes /api/vcs/* endpoints backed by the real .vcs/ store.
 */

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

const API = '/api/vcs'

async function apiFetch<T>(path: string): Promise<T> {
  const res = await fetch(`${API}${path}`)
  if (!res.ok) {
    const { error } = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(error)
  }
  return res.json()
}

async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!res.ok) {
    const { error } = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(error)
  }
  return res.json()
}

// ── Types ─────────────────────────────────────────────────────────────────

export interface VcsStatus {
  storePath: string
  initialised: boolean
  openStacks: number
}

export interface Change {
  change_id: string
  parent_id: string | null
  path: string
  op: 'create' | 'edit' | 'delete' | 'rename'
  diff_hash: string | null
  agent_id: string
  intent: {
    reason: string
    tool_call?: unknown
    task_ref?: string
  }
  created_at: number
}

export interface Candidate {
  stack_id: string
  change_id: string
  blob_hash: string | null
}

export interface Conflict {
  conflict_id: string
  view_id: string
  path: string
  candidates: Candidate[]
  resolution: unknown | null
}

// ── Resolution types ──────────────────────────────────────────────────────

export type Resolution =
  | { type: 'pick'; stack_id: string }
  | { type: 'merge'; content: string }

// ── Hooks ─────────────────────────────────────────────────────────────────

export function useVcsStatus() {
  return useQuery<VcsStatus>({
    queryKey: ['vcs', 'status'],
    queryFn: () => apiFetch('/status'),
    refetchInterval: 3000,
  })
}

export function useVcsFiles(viewId: string | null) {
  return useQuery<string[]>({
    queryKey: ['vcs', 'files', viewId],
    queryFn: () => apiFetch(`/view/${viewId}/files`),
    enabled: !!viewId,
    refetchInterval: 3000,
  })
}

export function useVcsLog(stackId: string | null) {
  return useQuery<Change[]>({
    queryKey: ['vcs', 'log', stackId],
    queryFn: () => apiFetch(`/stack/${stackId}/log`),
    enabled: !!stackId,
    refetchInterval: 3000,
  })
}

export function useVcsAllChanges() {
  return useQuery<Change[]>({
    queryKey: ['vcs', 'changes'],
    queryFn: () => apiFetch('/changes'),
    refetchInterval: 3000,
  })
}

export function useVcsConflicts(viewId: string | null) {
  return useQuery<Conflict[]>({
    queryKey: ['vcs', 'conflicts', viewId],
    queryFn: () => apiFetch(`/view/${viewId}/conflicts`),
    enabled: !!viewId,
    refetchInterval: 3000,
  })
}

export function useActiveView() {
  return useQuery<{ view_id: string; base_change_id: string; stack_ids: string[] } | null>({
    queryKey: ['vcs', 'active-view'],
    queryFn: () => apiFetch('/active-view'),
    refetchInterval: 3000,
  })
}

/**
 * Mutation hook for resolving a conflict.
 *
 * Usage:
 *   const resolve = useResolveConflict()
 *   resolve.mutate({ conflictId: 'abc', resolution: { type: 'pick', stack_id: 'xyz' } })
 *   resolve.mutate({ conflictId: 'abc', resolution: { type: 'merge', content: '…merged…' } })
 *
 * On success, all conflict queries are invalidated so the UI refreshes.
 */
export function useResolveConflict() {
  const qc = useQueryClient()

  return useMutation({
    mutationFn: async ({
      conflictId,
      resolution,
    }: {
      conflictId: string
      resolution: Resolution
    }) => {
      if (resolution.type === 'pick') {
        return apiPost(`/conflicts/${conflictId}/resolve`, { pick: resolution.stack_id })
      }
      // Custom merge — encode as base64
      const encoder = new TextEncoder()
      const bytes = encoder.encode(resolution.content)
      const b64 = btoa(String.fromCharCode(...bytes))
      return apiPost(`/conflicts/${conflictId}/resolve`, { merge_content_b64: b64 })
    },
    onSuccess: () => {
      // Refresh conflict list and active view
      qc.invalidateQueries({ queryKey: ['vcs', 'conflicts'] })
      qc.invalidateQueries({ queryKey: ['vcs', 'active-view'] })
    },
  })
}
