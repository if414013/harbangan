import { useState, useEffect } from 'react'
import { getQwenStatus, startQwenDeviceFlow, pollQwenDeviceCode, disconnectQwen } from '../lib/api'
import type { QwenStatus, QwenDeviceCodeResponse } from '../lib/api'
import { DeviceCodeDisplay } from './DeviceCodeDisplay'
import { useToast } from './Toast'

export function QwenSetup() {
  const { showToast } = useToast()
  const [status, setStatus] = useState<QwenStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [deviceAuth, setDeviceAuth] = useState<QwenDeviceCodeResponse | null>(null)
  const [starting, setStarting] = useState(false)

  function loadStatus() {
    getQwenStatus()
      .then(s => { setStatus(s); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadStatus() }, [])

  async function handleStart() {
    setStarting(true)
    try {
      const result = await startQwenDeviceFlow()
      setDeviceAuth(result)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Unknown error'
      showToast('Failed to start Qwen setup: ' + msg, 'error')
    } finally {
      setStarting(false)
    }
  }

  function handleComplete() {
    setDeviceAuth(null)
    showToast('Qwen Coder connected successfully', 'success')
    loadStatus()
  }

  function handleError(message: string) {
    showToast(message, 'error')
    setDeviceAuth(null)
  }

  async function handleDisconnect() {
    try {
      await disconnectQwen()
      showToast('Qwen Coder disconnected', 'success')
      loadStatus()
    } catch (err) {
      showToast(
        'Failed to disconnect: ' + (err instanceof Error ? err.message : 'Unknown error'),
        'error',
      )
    }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading Qwen status" />
  }

  if (deviceAuth) {
    return (
      <div className="card">
        <div className="card-header">
          <span className="card-title">{'> '}Qwen Setup</span>
        </div>
        <DeviceCodeDisplay
          userCode={deviceAuth.user_code}
          verificationUri={deviceAuth.verification_uri}
          verificationUriComplete={deviceAuth.verification_uri_complete ?? deviceAuth.verification_uri}
          deviceCode={deviceAuth.device_code}
          pollFn={pollQwenDeviceCode}
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
        <span className="card-title">{'> '}Qwen Coder</span>
        {status?.connected && !status.expired && (
          <span className="tag-ok">CONNECTED</span>
        )}
        {status?.connected && status.expired && (
          <span className="tag-warn">EXPIRED</span>
        )}
        {!status?.connected && (
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
          {status?.connected ? '$ reconnect' : '$ connect qwen'}
        </button>
        {status?.connected && (
          <button
            className="device-code-cancel"
            type="button"
            onClick={handleDisconnect}
          >
            disconnect
          </button>
        )}
      </div>
    </div>
  )
}
