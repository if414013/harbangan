import { authHeaders } from './auth'

const BASE = '/_ui/api'

export async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { ...authHeaders(), ...init?.headers },
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
}

export async function postSetup(data: {
  proxy_api_key: string
  kiro_refresh_token: string
  region: string
}): Promise<void> {
  const res = await fetch(`${BASE}/setup`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  })
  if (!res.ok) {
    const text = await res.text()
    throw new Error(text || `HTTP ${res.status}`)
  }
}

export async function getConfigSchema(): Promise<Record<string, unknown>> {
  return apiFetch<Record<string, unknown>>('/config/schema')
}

export async function checkSetupStatus(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE}/config`)
    if (!res.ok) return false
    const data = await res.json()
    return data.setup_complete !== false
  } catch {
    return false
  }
}
