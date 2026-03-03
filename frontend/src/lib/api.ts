import { authHeaders } from './auth'

const BASE = '/_ui/api'

export class ApiResponseError extends Error {
  readonly status: number
  readonly code: string | undefined

  constructor(status: number, message: string, code?: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

export async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    credentials: 'include',
    headers: { ...authHeaders(), ...init?.headers },
  })

  if (res.status === 401) {
    window.location.href = '/_ui/login'
    throw new ApiResponseError(401, 'Session expired')
  }

  if (!res.ok) {
    let body: { error?: string; message?: string } | undefined
    try { body = await res.json() } catch { /* not JSON */ }
    throw new ApiResponseError(
      res.status,
      body?.message || body?.error || `HTTP ${res.status}`,
      body?.error,
    )
  }

  if (res.status === 204) return undefined as T
  const text = await res.text()
  return text ? JSON.parse(text) as T : undefined as T
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'POST',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
}

export async function apiDelete(path: string): Promise<void> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'DELETE',
    credentials: 'include',
    headers: authHeaders(),
  })
  if (res.status === 401) {
    window.location.href = '/_ui/login'
    throw new ApiResponseError(401, 'Session expired')
  }
  if (!res.ok) {
    let body: { error?: string; message?: string } | undefined
    try { body = await res.json() } catch { /* not JSON */ }
    throw new ApiResponseError(
      res.status,
      body?.message || body?.error || `HTTP ${res.status}`,
      body?.error,
    )
  }
}

export async function checkSetupStatus(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE}/status`, { credentials: 'include' })
    if (!res.ok) return false
    const data = await res.json()
    return data.setup_complete === true
  } catch {
    return false
  }
}

export async function pollDeviceCode(deviceCodeId: string): Promise<DevicePollResponse> {
  return apiPost<DevicePollResponse>('/kiro/poll', { device_code_id: deviceCodeId })
}

// --- Types ---

export interface User {
  id: string
  email: string
  name: string
  picture_url: string | null
  role: 'admin' | 'user'
  last_login: string | null
  created_at: string
}

export interface ApiKeyInfo {
  id: string
  key_prefix: string
  label: string
  last_used: string | null
  created_at: string
}

export interface ApiKeyCreateResponse {
  id: string
  key: string
  key_prefix: string
  label: string
}

export interface KiroStatus {
  has_token: boolean
  expired: boolean
}

export interface DeviceCodeResponse {
  user_code: string
  verification_uri: string
  verification_uri_complete: string
  device_code_id: string
}

export interface DevicePollResponse {
  status: 'pending' | 'slow_down' | 'complete'
}

export interface DomainInfo {
  domain: string
  added_by: string | null
  created_at: string
}
