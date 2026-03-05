import { useState, useEffect } from 'react'
import { apiFetch, apiPost, apiDelete } from '../lib/api'
import type { ApiKeyInfo, ApiKeyCreateResponse } from '../lib/api'
import { useToast } from './Toast'

export function ApiKeyManager() {
  const { showToast } = useToast()
  const [keys, setKeys] = useState<ApiKeyInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [label, setLabel] = useState('')
  const [creating, setCreating] = useState(false)
  const [newKey, setNewKey] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  function loadKeys() {
    apiFetch<{ keys: ApiKeyInfo[] }>('/keys')
      .then(data => { setKeys(data.keys); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadKeys() }, [])

  async function handleCreate() {
    setCreating(true)
    try {
      const result = await apiPost<ApiKeyCreateResponse>('/keys', { label: label.trim() || 'default' })
      setNewKey(result.key)
      setLabel('')
      loadKeys()
    } catch (err) {
      showToast(
        'Failed to create key: ' + (err instanceof Error ? err.message : 'Unknown error'),
        'error',
      )
    } finally {
      setCreating(false)
    }
  }

  async function handleRevoke(id: string) {
    try {
      await apiDelete(`/keys/${id}`)
      showToast('API key revoked', 'success')
      loadKeys()
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : 'Failed to revoke key',
        'error',
      )
    }
  }

  async function handleCopy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch { /* ignore */ }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading API keys" />
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{'> '}api keys</span>
        <span className="card-subtitle">{keys.length}/10</span>
      </div>

      {newKey && (
        <div className="key-new-banner">
          <div className="key-new-label">New API key created — copy it now, it won't be shown again:</div>
          <div className="key-new-row">
            <code className="key-new-value" data-dd-privacy="mask">{newKey}</code>
            <button
              className="btn-reveal"
              type="button"
              onClick={() => handleCopy(newKey)}
              aria-live="polite"
              data-dd-action-name="copy-api-key"
            >
              {copied ? '[copied]' : '[copy]'}
            </button>
          </div>
          <button
            className="device-code-cancel"
            type="button"
            onClick={() => setNewKey(null)}
            style={{ marginTop: 8 }}
          >
            dismiss
          </button>
        </div>
      )}

      {keys.length > 0 && (
        <table className="data-table">
          <caption className="sr-only">API Keys</caption>
          <thead>
            <tr>
              <th scope="col">prefix</th>
              <th scope="col">label</th>
              <th scope="col">created</th>
              <th scope="col">last used</th>
              <th scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {keys.map(k => (
              <tr key={k.id}>
                <td>{k.key_prefix}...</td>
                <td style={{ color: 'var(--text-secondary)' }}>{k.label}</td>
                <td style={{ color: 'var(--text-secondary)' }}>{new Date(k.created_at).toLocaleDateString()}</td>
                <td style={{ color: 'var(--text-secondary)' }}>{k.last_used ? new Date(k.last_used).toLocaleDateString() : '\u2014'}</td>
                <td>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => handleRevoke(k.id)}
                    style={{ color: 'var(--red)' }}
                    aria-label={`Revoke API key ${k.key_prefix}`}
                  >
                    revoke
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {keys.length === 0 && !newKey && (
        <div className="empty-state">No API keys yet</div>
      )}

      {keys.length < 10 && (
        <div className="key-create-row">
          <input
            className="config-input"
            type="text"
            aria-label="API key label"
            placeholder="key label (optional)"
            value={label}
            onChange={e => setLabel(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') handleCreate() }}
          />
          <button
            className="btn-save"
            type="button"
            onClick={handleCreate}
            disabled={creating}
          >
            {creating ? 'creating...' : '$ generate key'}
          </button>
        </div>
      )}
    </div>
  )
}
