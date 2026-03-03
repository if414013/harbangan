import { useState, useEffect } from 'react'
import { apiFetch, apiPost, apiDelete } from '../lib/api'
import type { DomainInfo } from '../lib/api'
import { useToast } from './Toast'

export function DomainManager() {
  const { showToast } = useToast()
  const [domains, setDomains] = useState<DomainInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [newDomain, setNewDomain] = useState('')
  const [adding, setAdding] = useState(false)

  function loadDomains() {
    apiFetch<{ domains: DomainInfo[] }>('/domains')
      .then(data => { setDomains(data.domains); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadDomains() }, [])

  async function handleAdd() {
    const domain = newDomain.trim().toLowerCase()
    if (!domain) return
    setAdding(true)
    try {
      await apiPost('/domains', { domain })
      setNewDomain('')
      showToast(`Domain ${domain} added`, 'success')
      loadDomains()
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : 'Failed to add domain',
        'error',
      )
    } finally {
      setAdding(false)
    }
  }

  async function handleRemove(domain: string) {
    try {
      await apiDelete(`/domains/${encodeURIComponent(domain)}`)
      showToast(`Domain ${domain} removed`, 'success')
      loadDomains()
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : 'Failed to remove domain',
        'error',
      )
    }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading domains" />
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{'> '}allowed domains</span>
        <span className="card-subtitle">
          {domains.length === 0 ? 'open' : `${domains.length} domain${domains.length !== 1 ? 's' : ''}`}
        </span>
      </div>

      {domains.length === 0 && (
        <div className="empty-state" style={{ color: 'var(--yellow)' }}>
          No domain restrictions — any Google account can sign in
        </div>
      )}

      {domains.length > 0 && (
        <div className="domain-list">
          {domains.map(d => (
            <div key={d.domain} className="domain-item">
              <span className="domain-name">{d.domain}</span>
              <button
                className="device-code-cancel"
                type="button"
                onClick={() => handleRemove(d.domain)}
                style={{ color: 'var(--red)' }}
                aria-label={`Remove domain ${d.domain}`}
              >
                remove
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="key-create-row">
        <input
          className="config-input"
          type="text"
          aria-label="Domain name to allow"
          placeholder="example.com"
          value={newDomain}
          onChange={e => setNewDomain(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') handleAdd() }}
        />
        <button
          className="btn-save"
          type="button"
          onClick={handleAdd}
          disabled={adding || !newDomain.trim()}
        >
          {adding ? 'adding...' : '$ add domain'}
        </button>
      </div>
    </div>
  )
}
