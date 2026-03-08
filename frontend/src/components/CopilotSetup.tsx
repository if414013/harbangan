import { useState, useEffect } from 'react'
import { getCopilotStatus, startCopilotDeviceFlow, pollCopilotDeviceCode, disconnectCopilot } from '../lib/api'
import type { CopilotStatus, CopilotDeviceCodeResponse } from '../lib/api'
import { DeviceCodeDisplay } from './DeviceCodeDisplay'
import { useToast } from './Toast'

export function CopilotSetup() {
  const { showToast } = useToast()
  const [status, setStatus] = useState<CopilotStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [deviceAuth, setDeviceAuth] = useState<CopilotDeviceCodeResponse | null>(null)
  const [starting, setStarting] = useState(false)

  function loadStatus() {
    getCopilotStatus()
      .then(s => { setStatus(s); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadStatus() }, [])

  async function handleStart() {
    setStarting(true)
    try {
      const result = await startCopilotDeviceFlow()
      setDeviceAuth(result)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Unknown error'
      showToast('Failed to start Copilot setup: ' + msg, 'error')
    } finally {
      setStarting(false)
    }
  }

  function handleComplete() {
    setDeviceAuth(null)
    showToast('GitHub Copilot connected successfully', 'success')
    loadStatus()
  }

  function handleError(message: string) {
    showToast(message, 'error')
    setDeviceAuth(null)
  }

  async function handleDisconnect() {
    try {
      await disconnectCopilot()
      showToast('GitHub Copilot disconnected', 'success')
      loadStatus()
    } catch (err) {
      showToast(
        'Failed to disconnect: ' + (err instanceof Error ? err.message : 'Unknown error'),
        'error',
      )
    }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading Copilot status" />
  }

  if (deviceAuth) {
    return (
      <div className="card">
        <div className="card-header">
          <span className="card-title">{'> '}Copilot Setup</span>
        </div>
        <DeviceCodeDisplay
          userCode={deviceAuth.user_code}
          verificationUri={deviceAuth.verification_uri}
          verificationUriComplete={deviceAuth.verification_uri}
          deviceCode={deviceAuth.device_code}
          pollFn={pollCopilotDeviceCode}
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
        <span className="card-title">{'> '}GitHub Copilot</span>
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
      {status?.connected && status.github_username && (
        <div className="copilot-info">
          <span className="copilot-username">{status.github_username}</span>
          {status.copilot_plan && (
            <span className="copilot-plan">{status.copilot_plan}</span>
          )}
        </div>
      )}
      <div className="kiro-actions">
        <button
          className="btn-save"
          type="button"
          onClick={handleStart}
          disabled={starting}
        >
          {status?.connected ? '$ reconnect' : '$ connect github'}
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
