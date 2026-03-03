function getCsrfToken(): string | null {
  const match = document.cookie.match(/(?:^|;\s*)csrf_token=([^;]*)/)
  return match ? decodeURIComponent(match[1]) : null
}

export function authHeaders(): Record<string, string> {
  const csrf = getCsrfToken()
  return csrf ? { 'X-CSRF-Token': csrf } : {}
}
