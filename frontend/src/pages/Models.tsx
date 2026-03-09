import { useState, useEffect } from 'react'
import {
  getRegistryModels,
  updateModelEnabled,
  deleteRegistryModel,
  populateModels,
} from '../lib/api'
import type { RegistryModel } from '../lib/api'
import { useToast } from '../components/Toast'

interface ProviderGroup {
  providerId: string
  models: RegistryModel[]
}

function groupByProvider(models: RegistryModel[]): ProviderGroup[] {
  const map = new Map<string, RegistryModel[]>()
  for (const m of models) {
    const list = map.get(m.provider_id) ?? []
    list.push(m)
    map.set(m.provider_id, list)
  }
  return Array.from(map.entries())
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([providerId, models]) => ({ providerId, models }))
}

function ProviderSection({
  group,
  onToggle,
  onDelete,
  onPopulate,
}: {
  group: ProviderGroup
  onToggle: (id: string, enabled: boolean) => void
  onDelete: (id: string) => void
  onPopulate: (providerId: string) => void
}) {
  const [collapsed, setCollapsed] = useState(false)
  const enabledCount = group.models.filter(m => m.enabled).length

  function handleEnableAll() {
    for (const m of group.models) {
      if (!m.enabled) onToggle(m.id, true)
    }
  }

  function handleDisableAll() {
    for (const m of group.models) {
      if (m.enabled) onToggle(m.id, false)
    }
  }

  return (
    <div className={`config-group${collapsed ? ' collapsed' : ''}`}>
      <div
        className="config-group-header"
        onClick={() => setCollapsed(c => !c)}
        onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setCollapsed(c => !c) } }}
        tabIndex={0}
        role="button"
        aria-expanded={!collapsed}
      >
        <span>{group.providerId}</span>
        <span style={{ marginLeft: 'auto', fontSize: '0.62rem', color: 'var(--text-tertiary)', fontWeight: 400 }}>
          {enabledCount}/{group.models.length} enabled
        </span>
      </div>
      <div className="config-group-body">
        <div style={{ padding: '8px 16px', display: 'flex', gap: 8 }}>
          <button className="btn-reveal" type="button" onClick={e => { e.stopPropagation(); onPopulate(group.providerId) }}>
            $ populate
          </button>
          <button className="btn-reveal" type="button" onClick={handleEnableAll}>
            enable all
          </button>
          <button className="btn-reveal" type="button" onClick={handleDisableAll}>
            disable all
          </button>
        </div>
        <table className="data-table">
          <caption className="sr-only">Models for {group.providerId}</caption>
          <thead>
            <tr>
              <th scope="col">enabled</th>
              <th scope="col">prefixed id</th>
              <th scope="col">display name</th>
              <th scope="col">context</th>
              <th scope="col">source</th>
              <th scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {group.models.map(m => (
              <tr key={m.id}>
                <td>
                  <button
                    type="button"
                    className="role-badge"
                    onClick={() => onToggle(m.id, !m.enabled)}
                    aria-label={`Toggle ${m.prefixed_id} ${m.enabled ? 'off' : 'on'}`}
                    style={{
                      background: m.enabled ? 'var(--green-dim)' : 'var(--red-dim)',
                      color: m.enabled ? 'var(--green)' : 'var(--red)',
                    }}
                  >
                    {m.enabled ? 'on' : 'off'}
                  </button>
                </td>
                <td>{m.prefixed_id}</td>
                <td style={{ color: 'var(--text-secondary)' }}>{m.display_name}</td>
                <td style={{ color: 'var(--text-tertiary)' }}>{m.context_length.toLocaleString()}</td>
                <td>
                  <span className="mcp-type-badge">{m.source}</span>
                </td>
                <td>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => onDelete(m.id)}
                    aria-label={`Delete ${m.prefixed_id}`}
                    style={{ color: 'var(--red)' }}
                  >
                    delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

export function Models() {
  const { showToast } = useToast()
  const [models, setModels] = useState<RegistryModel[]>([])
  const [loading, setLoading] = useState(true)
  const [populating, setPopulating] = useState(false)

  function loadModels() {
    getRegistryModels()
      .then(data => { setModels(data.models); setLoading(false) })
      .catch(() => { setLoading(false) })
  }

  useEffect(() => { loadModels() }, [])

  async function handleToggle(id: string, enabled: boolean) {
    try {
      await updateModelEnabled(id, enabled)
      setModels(prev => prev.map(m => m.id === id ? { ...m, enabled } : m))
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to update model', 'error')
    }
  }

  async function handleDelete(id: string) {
    if (!confirm('Delete this model from the registry?')) return
    try {
      await deleteRegistryModel(id)
      showToast('Model deleted', 'success')
      setModels(prev => prev.filter(m => m.id !== id))
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to delete model', 'error')
    }
  }

  async function handlePopulate(providerId?: string) {
    setPopulating(true)
    try {
      const res = await populateModels(providerId)
      showToast(`Populated ${res.models_upserted} models`, 'success')
      loadModels()
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to populate models', 'error')
    } finally {
      setPopulating(false)
    }
  }

  const groups = groupByProvider(models)

  if (loading) {
    return (
      <>
        <h2 className="section-header">MODEL REGISTRY</h2>
        <div className="skeleton skeleton-block" role="status" aria-label="Loading models" />
      </>
    )
  }

  return (
    <>
      <h2 className="section-header">MODEL REGISTRY</h2>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
        <button className="btn-save" type="button" onClick={() => handlePopulate()} disabled={populating}>
          {populating ? 'populating...' : '$ populate all'}
        </button>
        <span className="card-subtitle">{models.length} models across {groups.length} providers</span>
      </div>
      {groups.length === 0 ? (
        <div className="card">
          <div className="empty-state">No models in registry. Click "populate all" to fetch models from connected providers.</div>
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {groups.map(g => (
            <ProviderSection
              key={g.providerId}
              group={g}
              onToggle={handleToggle}
              onDelete={handleDelete}
              onPopulate={handlePopulate}
            />
          ))}
        </div>
      )}
    </>
  )
}
