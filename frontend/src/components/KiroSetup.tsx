import { useState, useEffect } from 'react'
import { apiFetch, apiPost, apiDelete, pollDeviceCode } from '../lib/api'
import type { KiroStatus, DeviceCodeResponse } from '../lib/api'
import { DeviceCodeDisplay } from './DeviceCodeDisplay'
import { useToast } from './useToast'

export function KiroSetup() {
  const { showToast } = useToast()
  const [status, setStatus] = useState<KiroStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [deviceAuth, setDeviceAuth] = useState<DeviceCodeResponse | null>(null)
  const [starting, setStarting] = useState(false)
  const [ssoStartUrl, setSsoStartUrl] = useState('')
  const [ssoRegion, setSsoRegion] = useState('us-east-1')

  function loadStatus() {
    apiFetch<KiroStatus>('/kiro/status')
      .then(s => {
        setStatus(s)
        if (s.sso_start_url) setSsoStartUrl(s.sso_start_url)
        if (s.sso_region) setSsoRegion(s.sso_region)
        setLoading(false)
      })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadStatus() }, [])

  async function handleStart() {
    if (!ssoStartUrl.trim()) {
      showToast('SSO Start URL is required', 'error')
      return
    }
    setStarting(true)
    try {
      const result = await apiPost<DeviceCodeResponse>('/kiro/setup', {
        sso_start_url: ssoStartUrl.trim(),
        sso_region: ssoRegion.trim() || undefined,
      })
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
      <div className="kiro-sso-fields" style={{ display: 'flex', flexDirection: 'column', gap: '0.5rem', marginBottom: '0.75rem' }}>
        <label className="config-label" htmlFor="sso-start-url">
          SSO Start URL
        </label>
        <input
          id="sso-start-url"
          type="text"
          className="config-input"
          placeholder="https://d-xxxxxxxxxx.awsapps.com/start"
          value={ssoStartUrl}
          onChange={e => setSsoStartUrl(e.target.value)}
        />
        <label className="config-label" htmlFor="sso-region">
          SSO Region
        </label>
        <select
          id="sso-region"
          className="config-input"
          value={ssoRegion}
          onChange={e => setSsoRegion(e.target.value)}
        >
          <option value="us-east-1">us-east-1 (N. Virginia)</option>
          <option value="us-east-2">us-east-2 (Ohio)</option>
          <option value="us-west-2">us-west-2 (Oregon)</option>
          <option value="ca-central-1">ca-central-1 (Canada)</option>
          <option value="eu-west-1">eu-west-1 (Ireland)</option>
          <option value="eu-west-2">eu-west-2 (London)</option>
          <option value="eu-central-1">eu-central-1 (Frankfurt)</option>
          <option value="eu-north-1">eu-north-1 (Stockholm)</option>
          <option value="ap-southeast-1">ap-southeast-1 (Singapore)</option>
          <option value="ap-southeast-2">ap-southeast-2 (Sydney)</option>
          <option value="ap-northeast-1">ap-northeast-1 (Tokyo)</option>
          <option value="ap-northeast-2">ap-northeast-2 (Seoul)</option>
          <option value="ap-south-1">ap-south-1 (Mumbai)</option>
          <option value="sa-east-1">sa-east-1 (S. Paulo)</option>
          <option value="me-south-1">me-south-1 (Bahrain)</option>
          <option value="af-south-1">af-south-1 (Cape Town)</option>
          <option value="il-central-1">il-central-1 (Tel Aviv)</option>
        </select>
      </div>
      <div className="kiro-actions">
        <button
          className="btn-save"
          type="button"
          onClick={handleStart}
          disabled={starting || !ssoStartUrl.trim()}
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
