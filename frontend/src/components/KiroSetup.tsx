import { useState, useEffect } from 'react'
import { apiFetch, apiPost, apiDelete, pollDeviceCode } from '../lib/api'
import type { KiroStatus, DeviceCodeResponse } from '../lib/api'
import { DeviceCodeDisplay } from './DeviceCodeDisplay'
import { useToast } from './Toast'

export function KiroSetup() {
  const { showToast } = useToast()
  const [status, setStatus] = useState<KiroStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [deviceAuth, setDeviceAuth] = useState<DeviceCodeResponse | null>(null)
  const [starting, setStarting] = useState(false)

  function loadStatus() {
    apiFetch<KiroStatus>('/kiro/status')
      .then(s => { setStatus(s); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadStatus() }, [])

  async function handleStart() {
    setStarting(true)
    try {
      const result = await apiPost<DeviceCodeResponse>('/kiro/setup')
      setDeviceAuth(result)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Unknown error'
      showToast('Failed to start Kiro setup: ' + msg, 'error')
    } finally {
      setStarting(false)
    }
  }

  function handleComplete() {
    setDeviceAuth(null)
    showToast('Kiro token connected successfully', 'success')
    loadStatus()
  }

  function handleError(message: string) {
    showToast(message, 'error')
    setDeviceAuth(null)
  }

  async function handleRemove() {
    try {
      await apiDelete('/kiro/token')
      showToast('Kiro token removed', 'success')
      loadStatus()
    } catch (err) {
      showToast(
        'Failed to remove token: ' + (err instanceof Error ? err.message : 'Unknown error'),
        'error',
      )
    }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading Kiro status" />
  }

  if (deviceAuth) {
    return (
      <div className="card">
        <div className="card-header">
          <span className="card-title">{'> '}Kiro Setup</span>
        </div>
        <DeviceCodeDisplay
          userCode={deviceAuth.user_code}
          verificationUri={deviceAuth.verification_uri}
          verificationUriComplete={deviceAuth.verification_uri_complete}
          deviceCode={deviceAuth.device_code}
          pollFn={pollDeviceCode}
          onComplete={handleComplete}
          onError={handleError}
          onCancel={() => setDeviceAuth(null)}
        />
      </div>
    )
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{'> '}Kiro Connection</span>
        {status?.has_token && !status.expired && (
          <span className="tag-ok">CONNECTED</span>
        )}
        {status?.has_token && status.expired && (
          <span className="tag-warn">EXPIRED</span>
        )}
        {!status?.has_token && (
          <span className="tag-err">NOT CONNECTED</span>
        )}
      </div>
      <div className="kiro-actions">
        <button
          className="btn-save"
          type="button"
          onClick={handleStart}
          disabled={starting}
        >
          {status?.has_token ? '$ reconnect' : '$ setup kiro token'}
        </button>
        {status?.has_token && (
          <button
            className="device-code-cancel"
            type="button"
            onClick={handleRemove}
          >
            remove
          </button>
        )}
      </div>
    </div>
  )
}
