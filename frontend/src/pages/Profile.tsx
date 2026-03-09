import { useState, useEffect, useRef } from 'react'
import { KiroSetup } from '../components/KiroSetup'
import { CopilotSetup } from '../components/CopilotSetup'
import { QwenSetup } from '../components/QwenSetup'
import { ApiKeyManager } from '../components/ApiKeyManager'
import { useSession } from '../components/SessionGate'
import { useToast } from '../components/Toast'
import { getProvidersStatus, getProviderConnectUrl, disconnectProvider } from '../lib/api'
import type { ProvidersStatusResponse } from '../lib/api'

const PROVIDERS = ['anthropic', 'gemini', 'openai_codex'] as const

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  openai_codex: 'OpenAI Codex',
}

function providerDisplayName(id: string): string {
  return PROVIDER_DISPLAY_NAMES[id] ?? id.charAt(0).toUpperCase() + id.slice(1)
}

const RELAY_TIMEOUT_MS = 10 * 60 * 1000

interface RelayModalProps {
  provider: string
  relayScriptUrl: string
  onConnected: () => void
  onClose: () => void
}

function RelayModal({ provider, relayScriptUrl, onConnected, onClose }: RelayModalProps) {
  const [copied, setCopied] = useState(false)
  const [timedOut, setTimedOut] = useState(false)
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const mountedRef = useRef(true)

  const curlCommand = `curl -fsSL '${relayScriptUrl}' | sh`

  useEffect(() => {
    mountedRef.current = true

    pollRef.current = setInterval(async () => {
      if (!mountedRef.current) return
      try {
        const status = await getProvidersStatus()
        if (!mountedRef.current) return
        const p = status.providers[provider]
        if (p?.connected) {
          onConnected()
        }
      } catch {
        // ignore poll errors
      }
    }, 2000)

    timeoutRef.current = setTimeout(() => {
      if (!mountedRef.current) return
      setTimedOut(true)
      if (pollRef.current) clearInterval(pollRef.current)
    }, RELAY_TIMEOUT_MS)

    return () => {
      mountedRef.current = false
      if (pollRef.current) clearInterval(pollRef.current)
      if (timeoutRef.current) clearTimeout(timeoutRef.current)
    }
  }, [provider, onConnected])

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(curlCommand)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // ignore
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-box relay-modal" onClick={e => e.stopPropagation()}>
        <h3>connect {provider}</h3>
        {timedOut ? (
          <>
            <p className="relay-timeout">Connection timed out. Click connect to try again.</p>
            <div className="modal-actions">
              <button type="button" onClick={onClose}>$ close</button>
            </div>
          </>
        ) : (
          <>
            <p>Run this in your terminal:</p>
            <div className="relay-command-wrap">
              <code className="relay-command">{curlCommand}</code>
              <button type="button" className="relay-copy-btn" onClick={handleCopy}>
                {copied ? '[copied]' : '[copy]'}
              </button>
            </div>
            <div className="device-code-polling">
              <span className="cursor" />
              waiting for authorization...
            </div>
            <div className="modal-actions">
              <button type="button" onClick={onClose}>$ cancel</button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}

interface ProviderCardProps {
  provider: string
  connected: boolean
  email?: string
  onRefresh: () => void
}

function ProviderCard({ provider, connected, email, onRefresh }: ProviderCardProps) {
  const { showToast } = useToast()
  const [connecting, setConnecting] = useState(false)
  const [relayUrl, setRelayUrl] = useState<string | null>(null)

  async function handleConnect() {
    setConnecting(true)
    try {
      const result = await getProviderConnectUrl(provider)
      setRelayUrl(result.relay_script_url)
    } catch (err) {
      showToast('Failed to start connect: ' + (err instanceof Error ? err.message : 'Unknown error'), 'error')
    } finally {
      setConnecting(false)
    }
  }

  async function handleDisconnect() {
    try {
      await disconnectProvider(provider)
      showToast(`${provider} disconnected`, 'success')
      onRefresh()
    } catch (err) {
      showToast('Failed to disconnect: ' + (err instanceof Error ? err.message : 'Unknown error'), 'error')
    }
  }

  function handleConnected() {
    setRelayUrl(null)
    showToast(`${provider} connected`, 'success')
    onRefresh()
  }

  return (
    <>
      <div className="card provider-card">
        <div className="card-header">
          <span className="card-title">{'> '}{providerDisplayName(provider)}</span>
          {connected ? (
            <span className="tag-ok">CONNECTED</span>
          ) : (
            <span className="tag-err">NOT CONNECTED</span>
          )}
        </div>
        {connected && email && (
          <div className="provider-email">{email}</div>
        )}
        <div className="kiro-actions">
          {connected ? (
            <button className="device-code-cancel" type="button" onClick={handleDisconnect}>
              $ disconnect
            </button>
          ) : (
            <button className="btn-save" type="button" onClick={handleConnect} disabled={connecting}>
              {connecting ? '...' : '$ connect'}
            </button>
          )}
        </div>
      </div>
      {relayUrl && (
        <RelayModal
          provider={provider}
          relayScriptUrl={relayUrl}
          onConnected={handleConnected}
          onClose={() => setRelayUrl(null)}
        />
      )}
    </>
  )
}

interface TreeNodeProps {
  label: string
  last?: boolean
  children: React.ReactNode
}

function TreeNode({ label, last, children }: TreeNodeProps) {
  const [open, setOpen] = useState(false)

  return (
    <div className={`tree-node${last ? ' tree-node-last' : ''}`}>
      <button
        type="button"
        className="tree-node-toggle"
        onClick={() => setOpen(o => !o)}
        aria-expanded={open}
      >
        <span className="tree-branch">{last ? '└' : '├'}</span>
        <span className="tree-arrow">{open ? '▼' : '▶'}</span>
        <span className="tree-label">{label}</span>
      </button>
      {open && (
        <div className="tree-node-content">
          <div className={`tree-node-line${last ? ' tree-node-line-hidden' : ''}`} />
          <div className="tree-node-body">
            {children}
          </div>
        </div>
      )}
    </div>
  )
}

export function Profile() {
  const { user } = useSession()
  const [providerStatus, setProviderStatus] = useState<ProvidersStatusResponse | null>(null)

  function loadProviders() {
    getProvidersStatus()
      .then(setProviderStatus)
      .catch(() => {})
  }

  useEffect(() => { loadProviders() }, [])

  return (
    <>
      <h2 className="section-header">PROFILE</h2>
      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">{'> '}Account</span>
          <span
            style={{
              fontSize: '0.55rem',
              fontFamily: 'var(--font-mono)',
              padding: '1px 5px',
              borderRadius: 'var(--radius-sm)',
              background: user.role === 'admin' ? 'var(--green-dim)' : 'var(--blue-dim)',
              color: user.role === 'admin' ? 'var(--green)' : 'var(--blue)',
              whiteSpace: 'nowrap',
            }}
          >
            {user.role}
          </span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '4px 0' }}>
          {user.picture_url && (
            <img
              src={user.picture_url}
              alt=""
              style={{
                width: 32,
                height: 32,
                borderRadius: 'var(--radius)',
                opacity: 0.85,
              }}
            />
          )}
          <div>
            <div style={{ fontSize: '0.82rem', color: 'var(--text)', fontWeight: 500 }}>{user.name}</div>
            <div style={{ fontSize: '0.72rem', color: 'var(--text-tertiary)' }}>{user.email}</div>
          </div>
        </div>
      </div>

      <h2 className="section-header">API KEYS</h2>
      <div className="mb-24">
        <ApiKeyManager />
      </div>

      <h2 className="section-header">PROVIDERS</h2>
      <div className="provider-tree">
        <TreeNode label="Kiro">
          <KiroSetup />
        </TreeNode>
        <TreeNode label="github copilot">
          <CopilotSetup />
        </TreeNode>
        <TreeNode label="qwen coder">
          <QwenSetup />
        </TreeNode>
        {PROVIDERS.map((p, i) => {
          const info = providerStatus?.providers[p]
          return (
            <TreeNode key={p} label={providerDisplayName(p)} last={i === PROVIDERS.length - 1}>
              <ProviderCard
                provider={p}
                connected={info?.connected ?? false}
                email={info?.email}
                onRefresh={loadProviders}
              />
            </TreeNode>
          )
        })}
      </div>
    </>
  )
}
