import { useState, useEffect } from 'react'
import {
  getMcpClients,
  createMcpClient,
  updateMcpClient,
  deleteMcpClient,
  reconnectMcpClient,
} from '../lib/api'
import type { McpClientState, McpTool } from '../lib/api'
import { useToast } from '../components/Toast'

// --- Client List ---

function McpClientList() {
  const { showToast } = useToast()
  const [clients, setClients] = useState<McpClientState[]>([])
  const [loading, setLoading] = useState(true)
  const [showForm, setShowForm] = useState(false)
  const [editId, setEditId] = useState<string | null>(null)
  const [selectedClient, setSelectedClient] = useState<McpClientState | null>(null)
  const [connectionType, setConnectionType] = useState<'http' | 'sse' | 'stdio'>('http')
  const [form, setForm] = useState({
    name: '',
    connection_string: '',
    auth_type: 'none' as 'none' | 'headers',
    headers: '' ,
    tools_to_execute: '*',
    is_ping_available: true,
    tool_sync_interval_secs: 0,
    enabled: true,
    // stdio fields
    stdio_command: '',
    stdio_args: '',
    stdio_envs: '',
  })

  function loadClients() {
    getMcpClients()
      .then(data => { setClients(data); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadClients() }, [])

  function resetForm() {
    setForm({
      name: '', connection_string: '', auth_type: 'none', headers: '',
      tools_to_execute: '*', is_ping_available: true, tool_sync_interval_secs: 0, enabled: true,
      stdio_command: '', stdio_args: '', stdio_envs: '',
    })
    setConnectionType('http')
    setEditId(null)
    setShowForm(false)
  }

  function startEdit(c: McpClientState) {
    const cfg = c.config
    setConnectionType(cfg.connection_type)
    setForm({
      name: cfg.name,
      connection_string: cfg.connection_string || '',
      auth_type: cfg.auth_type,
      headers: '',
      tools_to_execute: cfg.tools_to_execute.join(', '),
      is_ping_available: cfg.is_ping_available,
      tool_sync_interval_secs: cfg.tool_sync_interval_secs,
      enabled: cfg.enabled,
      stdio_command: cfg.stdio_config?.command || '',
      stdio_args: cfg.stdio_config?.args.join(', ') || '',
      stdio_envs: cfg.stdio_config ? Object.entries(cfg.stdio_config.envs).map(([k, v]) => `${k}=${v}`).join('\n') : '',
    })
    setEditId(cfg.id)
    setShowForm(true)
  }

  function buildPayload(): Record<string, unknown> {
    const tools = form.tools_to_execute.split(',').map(s => s.trim()).filter(Boolean)
    const payload: Record<string, unknown> = {
      name: form.name,
      connection_type: connectionType,
      auth_type: form.auth_type,
      tools_to_execute: tools.length === 0 ? ['*'] : tools,
      is_ping_available: form.is_ping_available,
      tool_sync_interval_secs: form.tool_sync_interval_secs,
      enabled: form.enabled,
    }

    if (connectionType === 'stdio') {
      const envEntries = form.stdio_envs.split('\n').filter(Boolean).map(line => {
        const idx = line.indexOf('=')
        return idx > 0 ? [line.slice(0, idx), line.slice(idx + 1)] : null
      }).filter(Boolean) as [string, string][]
      payload.stdio_config = {
        command: form.stdio_command,
        args: form.stdio_args.split(',').map(s => s.trim()).filter(Boolean),
        envs: Object.fromEntries(envEntries),
      }
    } else {
      payload.connection_string = form.connection_string
    }

    if (form.auth_type === 'headers' && form.headers.trim()) {
      try {
        payload.headers = JSON.parse(form.headers)
      } catch {
        payload.headers = {}
      }
    }

    return payload
  }

  async function handleSubmit() {
    if (!form.name.trim()) {
      showToast('Name is required', 'error')
      return
    }
    if (connectionType !== 'stdio' && !form.connection_string.trim()) {
      showToast('URL is required for HTTP/SSE connections', 'error')
      return
    }
    if (connectionType === 'stdio' && !form.stdio_command.trim()) {
      showToast('Command is required for STDIO connections', 'error')
      return
    }
    try {
      const payload = buildPayload()
      if (editId) {
        await updateMcpClient(editId, payload)
        showToast('Client updated', 'success')
      } else {
        await createMcpClient(payload)
        showToast('Client created', 'success')
      }
      resetForm()
      loadClients()
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to save client', 'error')
    }
  }

  async function handleDelete(id: string) {
    if (!confirm('Delete this MCP client? This cannot be undone.')) return
    try {
      await deleteMcpClient(id)
      showToast('Client deleted', 'success')
      if (selectedClient?.config.id === id) setSelectedClient(null)
      loadClients()
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to delete client', 'error')
    }
  }

  async function handleReconnect(id: string) {
    try {
      await reconnectMcpClient(id)
      showToast('Reconnecting...', 'success')
      loadClients()
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to reconnect', 'error')
    }
  }

  async function handleToggle(c: McpClientState) {
    try {
      await updateMcpClient(c.config.id, { enabled: !c.config.enabled })
      showToast(`Client ${c.config.enabled ? 'disabled' : 'enabled'}`, 'success')
      loadClients()
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to toggle client', 'error')
    }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading MCP clients" />
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{'> '}clients</span>
        <span className="card-subtitle">{clients.length} total</span>
      </div>
      {clients.length === 0 && !showForm ? (
        <div className="empty-state">No MCP clients configured</div>
      ) : (
        <table className="data-table">
          <caption className="sr-only">MCP Clients</caption>
          <thead>
            <tr>
              <th scope="col">name</th>
              <th scope="col">type</th>
              <th scope="col">status</th>
              <th scope="col">tools</th>
              <th scope="col">enabled</th>
              <th scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {clients.map(c => (
              <tr
                key={c.config.id}
                onClick={() => setSelectedClient(c)}
                onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setSelectedClient(c) } }}
                tabIndex={0}
                role="button"
                aria-label={`View details for ${c.config.name}`}
                style={{ cursor: 'pointer' }}
              >
                <td>{c.config.name}</td>
                <td>
                  <span className="mcp-type-badge">{c.config.connection_type}</span>
                </td>
                <td>
                  <span className={`mcp-status mcp-status-${c.connection_state}`}>
                    {c.connection_state}
                  </span>
                </td>
                <td>{c.tools.length}</td>
                <td>
                  <button
                    type="button"
                    className="role-badge"
                    onClick={e => { e.stopPropagation(); handleToggle(c) }}
                    aria-label={`Toggle client ${c.config.name} ${c.config.enabled ? 'off' : 'on'}`}
                    style={{
                      background: c.config.enabled ? 'var(--green-dim)' : 'var(--red-dim)',
                      color: c.config.enabled ? 'var(--green)' : 'var(--red)',
                    }}
                  >
                    {c.config.enabled ? 'on' : 'off'}
                  </button>
                </td>
                <td style={{ display: 'flex', gap: 8 }}>
                  <button className="device-code-cancel" type="button" onClick={e => { e.stopPropagation(); startEdit(c) }} aria-label={`Edit client ${c.config.name}`}>
                    edit
                  </button>
                  <button className="device-code-cancel" type="button" onClick={e => { e.stopPropagation(); handleReconnect(c.config.id) }} aria-label={`Reconnect client ${c.config.name}`}>
                    reconnect
                  </button>
                  <button className="device-code-cancel" type="button" onClick={e => { e.stopPropagation(); handleDelete(c.config.id) }} aria-label={`Delete client ${c.config.name}`} style={{ color: 'var(--red)' }}>
                    delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {showForm && (
        <div className="guardrails-form">
          <div className="guardrails-form-grid">
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="mcp-name">Name</label>
              <input id="mcp-name" className="config-input" autoFocus value={form.name} onChange={e => setForm(f => ({ ...f, name: e.target.value }))} />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="mcp-type">Connection Type</label>
              <select id="mcp-type" className="config-input" value={connectionType} onChange={e => setConnectionType(e.target.value as 'http' | 'sse' | 'stdio')}>
                <option value="http">HTTP</option>
                <option value="sse">SSE</option>
                <option value="stdio">STDIO</option>
              </select>
            </div>

            {connectionType !== 'stdio' ? (
              <div className="guardrails-form-field" style={{ gridColumn: '1 / -1' }}>
                <label className="config-label" htmlFor="mcp-url">URL</label>
                <input id="mcp-url" className="config-input" value={form.connection_string} onChange={e => setForm(f => ({ ...f, connection_string: e.target.value }))} placeholder="https://mcp-server.example.com" />
              </div>
            ) : (
              <>
                <div className="guardrails-form-field">
                  <label className="config-label" htmlFor="mcp-cmd">Command</label>
                  <input id="mcp-cmd" className="config-input" value={form.stdio_command} onChange={e => setForm(f => ({ ...f, stdio_command: e.target.value }))} placeholder="npx" />
                </div>
                <div className="guardrails-form-field">
                  <label className="config-label" htmlFor="mcp-args">Args (comma-separated)</label>
                  <input id="mcp-args" className="config-input" value={form.stdio_args} onChange={e => setForm(f => ({ ...f, stdio_args: e.target.value }))} placeholder="-y, @modelcontextprotocol/server-filesystem" />
                </div>
                <div className="guardrails-form-field" style={{ gridColumn: '1 / -1' }}>
                  <label className="config-label" htmlFor="mcp-envs">Environment (KEY=VALUE, one per line)</label>
                  <textarea id="mcp-envs" className="config-input guardrails-cel-input" value={form.stdio_envs} onChange={e => setForm(f => ({ ...f, stdio_envs: e.target.value }))} rows={2} placeholder={'PATH=/usr/bin\nNODE_ENV=production'} />
                </div>
              </>
            )}

            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="mcp-auth">Auth Type</label>
              <select id="mcp-auth" className="config-input" value={form.auth_type} onChange={e => setForm(f => ({ ...f, auth_type: e.target.value as 'none' | 'headers' }))}>
                <option value="none">None</option>
                <option value="headers">Headers</option>
              </select>
            </div>

            {form.auth_type === 'headers' && (
              <div className="guardrails-form-field">
                <label className="config-label" htmlFor="mcp-headers">Headers (JSON)</label>
                <textarea id="mcp-headers" className="config-input guardrails-cel-input" value={form.headers} onChange={e => setForm(f => ({ ...f, headers: e.target.value }))} rows={2} placeholder={'{"Authorization": "Bearer ..."}'} />
              </div>
            )}

            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="mcp-tools">Tools Filter</label>
              <input id="mcp-tools" className="config-input" value={form.tools_to_execute} onChange={e => setForm(f => ({ ...f, tools_to_execute: e.target.value }))} placeholder="* (all tools)" />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="mcp-sync">Tool Sync Interval (s)</label>
              <input id="mcp-sync" className="config-input" type="number" min={0} value={form.tool_sync_interval_secs} onChange={e => setForm(f => ({ ...f, tool_sync_interval_secs: Number(e.target.value) }))} />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label">
                <input type="checkbox" checked={form.is_ping_available} onChange={e => setForm(f => ({ ...f, is_ping_available: e.target.checked }))} style={{ marginRight: 8 }} />
                Ping Available
              </label>
            </div>
            <div className="guardrails-form-field">
              <label className="config-label">
                <input type="checkbox" checked={form.enabled} onChange={e => setForm(f => ({ ...f, enabled: e.target.checked }))} style={{ marginRight: 8 }} />
                Enabled
              </label>
            </div>
          </div>
          <div className="guardrails-form-actions">
            <button className="btn-save" type="button" onClick={handleSubmit}>
              {editId ? '$ update' : '$ create'}
            </button>
            <button className="device-code-cancel" type="button" onClick={resetForm}>
              cancel
            </button>
          </div>
        </div>
      )}

      {!showForm && (
        <div style={{ marginTop: 12 }}>
          <button className="btn-save" type="button" onClick={() => setShowForm(true)}>
            $ new client
          </button>
        </div>
      )}

      {selectedClient && (
        <McpClientDetail client={selectedClient} onClose={() => setSelectedClient(null)} onRefresh={loadClients} />
      )}
    </div>
  )
}

// --- Client Detail Panel ---

interface McpClientDetailProps {
  client: McpClientState
  onClose: () => void
  onRefresh: () => void
}

function McpClientDetail({ client, onClose, onRefresh }: McpClientDetailProps) {
  const { showToast } = useToast()
  const [tools, setTools] = useState<McpTool[]>(client.tools)

  useEffect(() => {
    setTools(client.tools)
  }, [client])

  async function handleReconnect() {
    try {
      await reconnectMcpClient(client.config.id)
      showToast('Reconnecting...', 'success')
      onRefresh()
    } catch (err) {
      showToast(err instanceof Error ? err.message : 'Failed to reconnect', 'error')
    }
  }

  const cfg = client.config

  return (
    <div className="mcp-detail-panel">
      <div className="card-header">
        <span className="card-title">{'> '}{cfg.name}</span>
        <button className="device-code-cancel" type="button" onClick={onClose}>close</button>
      </div>
      <div className="mcp-detail-grid">
        <div className="mcp-detail-item">
          <span className="mcp-detail-label">type</span>
          <span className="mcp-type-badge">{cfg.connection_type}</span>
        </div>
        <div className="mcp-detail-item">
          <span className="mcp-detail-label">status</span>
          <span className={`mcp-status mcp-status-${client.connection_state}`}>{client.connection_state}</span>
        </div>
        {cfg.connection_string && (
          <div className="mcp-detail-item" style={{ gridColumn: '1 / -1' }}>
            <span className="mcp-detail-label">url</span>
            <span style={{ fontSize: '0.72rem', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', wordBreak: 'break-all' }}>{cfg.connection_string}</span>
          </div>
        )}
        {cfg.stdio_config && (
          <div className="mcp-detail-item" style={{ gridColumn: '1 / -1' }}>
            <span className="mcp-detail-label">command</span>
            <span style={{ fontSize: '0.72rem', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
              {cfg.stdio_config.command} {cfg.stdio_config.args.join(' ')}
            </span>
          </div>
        )}
        <div className="mcp-detail-item">
          <span className="mcp-detail-label">auth</span>
          <span style={{ fontSize: '0.72rem', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>{cfg.auth_type}</span>
        </div>
        <div className="mcp-detail-item">
          <span className="mcp-detail-label">ping</span>
          <span style={{ fontSize: '0.72rem', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>{cfg.is_ping_available ? 'yes' : 'no'}</span>
        </div>
        {client.last_error && (
          <div className="mcp-detail-item" style={{ gridColumn: '1 / -1' }}>
            <span className="mcp-detail-label">last error</span>
            <span style={{ fontSize: '0.72rem', color: 'var(--red)', fontFamily: 'var(--font-mono)' }}>{client.last_error}</span>
          </div>
        )}
      </div>
      <div className="guardrails-form-actions">
        <button className="btn-save" type="button" onClick={handleReconnect}>$ reconnect</button>
      </div>

      {tools.length > 0 && (
        <div style={{ marginTop: 12 }}>
          <div className="mcp-detail-label" style={{ marginBottom: 8 }}>tools ({tools.length})</div>
          <div className="mcp-tools-list">
            {tools.map(t => (
              <div key={t.name} className="mcp-tool-item">
                <span className="mcp-tool-name">{t.name}</span>
                {t.description && <span className="mcp-tool-desc">{t.description}</span>}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

// --- Main Page ---

export function McpClients() {
  return (
    <>
      <h2 className="section-header">MCP SERVERS</h2>
      <McpClientList />
    </>
  )
}
