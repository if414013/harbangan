import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { changePassword } from '../lib/api'
import { useSession } from '../components/SessionGate'

export function PasswordChange() {
  const navigate = useNavigate()
  const { user } = useSession()
  const isInitialSetup = user.auth_method !== 'password'
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  function validate(): string | null {
    if (newPassword.length < 8) return 'New password must be at least 8 characters'
    if (newPassword !== confirmPassword) return 'Passwords do not match'
    return null
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const validationError = validate()
    if (validationError) {
      setError(validationError)
      return
    }
    setError(null)
    setSubmitting(true)
    try {
      await changePassword(isInitialSetup ? '' : currentPassword, newPassword)
      navigate('/', { replace: true })
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to change password')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="auth-overlay">
      <div className="auth-card">
        <div className="auth-logo">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--bg)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>
          </svg>
        </div>
        <h2><span aria-hidden="true">{'> '}</span>{isInitialSetup ? 'SET PASSWORD' : 'CHANGE PASSWORD'}<span className="cursor" aria-hidden="true" /></h2>
        <p>{isInitialSetup ? 'create a password for your account' : 'you must update your password to continue'}</p>

        {error && (
          <div className="login-error" role="alert" aria-live="assertive">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit}>
          {!isInitialSetup && (
            <input
              className="auth-input"
              type="password"
              placeholder="current password"
              value={currentPassword}
              onChange={e => setCurrentPassword(e.target.value)}
              autoComplete="current-password"
              required
              autoFocus
            />
          )}
          <input
            className="auth-input"
            type="password"
            placeholder="new password (min 8 chars)"
            value={newPassword}
            onChange={e => setNewPassword(e.target.value)}
            autoComplete="new-password"
            required
            minLength={8}
          />
          <input
            className="auth-input"
            type="password"
            placeholder="confirm new password"
            value={confirmPassword}
            onChange={e => setConfirmPassword(e.target.value)}
            autoComplete="new-password"
            required
            minLength={8}
          />
          <button className="auth-submit" type="submit" disabled={submitting}>
            {submitting
              ? (isInitialSetup ? '$ setting...' : '$ updating...')
              : (isInitialSetup ? '$ set password' : '$ update password')}
          </button>
        </form>
      </div>
    </div>
  )
}
