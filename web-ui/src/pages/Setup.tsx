import { useState, type FormEvent } from 'react'
import { postSetup } from '../lib/api'
import { useToast } from '../components/Toast'

interface SetupProps {
  onComplete: () => void
}

export function Setup({ onComplete }: SetupProps) {
  const { showToast } = useToast()
  const [apiKey, setApiKey] = useState('')
  const [refreshToken, setRefreshToken] = useState('')
  const [region, setRegion] = useState('us-east-1')
  const [showPassword, setShowPassword] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [errors, setErrors] = useState<Record<string, string>>({})

  function validate(): boolean {
    const next: Record<string, string> = {}
    if (!apiKey.trim()) {
      next.apiKey = 'Required'
    } else if (apiKey.trim().length < 8) {
      next.apiKey = 'Must be at least 8 characters'
    }
    if (!refreshToken.trim()) {
      next.refreshToken = 'Required'
    }
    setErrors(next)
    return Object.keys(next).length === 0
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault()
    if (!validate()) return
    setSubmitting(true)
    try {
      await postSetup({
        proxy_api_key: apiKey.trim(),
        kiro_refresh_token: refreshToken.trim(),
        region,
      })
      showToast('Setup complete! Redirecting...', 'success')
      setTimeout(onComplete, 600)
    } catch (err) {
      showToast('Setup failed: ' + (err instanceof Error ? err.message : 'Unknown error'), 'error')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="auth-overlay">
      <form
        className="auth-card"
        onSubmit={handleSubmit}
        style={{ width: 440, textAlign: 'left' }}
      >
        <div style={{ textAlign: 'center', marginBottom: 24 }}>
          <div className="auth-logo">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#101014" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 2L2 7l10 5 10-5-10-5z" />
              <path d="M2 17l10 5 10-5" />
              <path d="M2 12l10 5 10-5" />
            </svg>
          </div>
          <h2>Kiro Gateway</h2>
          <p style={{ margin: 0 }}>Welcome! Let's get your gateway configured.</p>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
          {/* Gateway Password */}
          <div>
            <label
              htmlFor="setup-api-key"
              style={{
                display: 'block',
                fontSize: '0.78rem',
                fontWeight: 500,
                color: 'var(--text-secondary)',
                marginBottom: 6,
              }}
            >
              Gateway Password
            </label>
            <div style={{ position: 'relative' }}>
              <input
                id="setup-api-key"
                className="auth-input"
                type={showPassword ? 'text' : 'password'}
                placeholder="Enter a password (min 8 characters)"
                autoComplete="new-password"
                value={apiKey}
                onChange={e => { setApiKey(e.target.value); setErrors(p => ({ ...p, apiKey: '' })) }}
                style={{ marginBottom: 0, paddingRight: 60 }}
                autoFocus
              />
              <button
                type="button"
                onClick={() => setShowPassword(v => !v)}
                style={{
                  position: 'absolute',
                  right: 8,
                  top: '50%',
                  transform: 'translateY(-50%)',
                  background: 'none',
                  border: '1px solid var(--border)',
                  color: 'var(--text-tertiary)',
                  padding: '3px 8px',
                  borderRadius: 'var(--radius-sm)',
                  cursor: 'pointer',
                  fontSize: '0.65rem',
                  fontFamily: 'var(--font-mono)',
                }}
              >
                {showPassword ? 'hide' : 'show'}
              </button>
            </div>
            {errors.apiKey && (
              <span style={{ fontSize: '0.72rem', color: 'var(--red)', marginTop: 4, display: 'block' }}>
                {errors.apiKey}
              </span>
            )}
            <span style={{ fontSize: '0.68rem', color: 'var(--text-tertiary)', marginTop: 6, display: 'block', lineHeight: 1.4 }}>
              This password protects access to your gateway. You'll use it to authenticate API requests.
            </span>
          </div>

          {/* Kiro Refresh Token */}
          <div>
            <label
              htmlFor="setup-refresh-token"
              style={{
                display: 'block',
                fontSize: '0.78rem',
                fontWeight: 500,
                color: 'var(--text-secondary)',
                marginBottom: 6,
              }}
            >
              Kiro Refresh Token
            </label>
            <textarea
              id="setup-refresh-token"
              className="auth-input"
              placeholder="Paste your refresh token here"
              value={refreshToken}
              onChange={e => { setRefreshToken(e.target.value); setErrors(p => ({ ...p, refreshToken: '' })) }}
              rows={3}
              style={{
                marginBottom: 0,
                resize: 'vertical',
                minHeight: 60,
                lineHeight: 1.4,
              }}
            />
            {errors.refreshToken && (
              <span style={{ fontSize: '0.72rem', color: 'var(--red)', marginTop: 4, display: 'block' }}>
                {errors.refreshToken}
              </span>
            )}
            <span style={{ fontSize: '0.68rem', color: 'var(--text-tertiary)', marginTop: 6, display: 'block', lineHeight: 1.4 }}>
              Run <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--amber)', fontSize: '0.66rem' }}>kiro login</code> in
              your terminal, then find your refresh token
              in <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--amber)', fontSize: '0.66rem' }}>~/.kiro/data.db</code>.
            </span>
          </div>

          {/* AWS Region */}
          <div>
            <label
              htmlFor="setup-region"
              style={{
                display: 'block',
                fontSize: '0.78rem',
                fontWeight: 500,
                color: 'var(--text-secondary)',
                marginBottom: 6,
              }}
            >
              AWS Region
            </label>
            <select
              id="setup-region"
              className="auth-input"
              value={region}
              onChange={e => setRegion(e.target.value)}
              style={{ marginBottom: 0, cursor: 'pointer' }}
            >
              <option value="us-east-1">us-east-1</option>
              <option value="us-west-2">us-west-2</option>
              <option value="eu-west-1">eu-west-1</option>
            </select>
          </div>
        </div>

        <button
          className="auth-submit"
          type="submit"
          disabled={submitting}
          style={{ marginTop: 28, opacity: submitting ? 0.7 : 1 }}
        >
          {submitting ? 'Setting up...' : 'Complete Setup'}
        </button>
      </form>
    </div>
  )
}
