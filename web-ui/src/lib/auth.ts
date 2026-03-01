export function getApiKey(): string | null {
  return sessionStorage.getItem('apiKey')
}

export function setApiKey(key: string): void {
  sessionStorage.setItem('apiKey', key)
}

export function authHeaders(): Record<string, string> {
  const key = getApiKey()
  return key ? { Authorization: `Bearer ${key}` } : {}
}
