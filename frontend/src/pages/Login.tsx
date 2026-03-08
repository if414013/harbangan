import { useEffect, useState } from 'react'
import { useSearchParams, useNavigate } from 'react-router-dom'

const ERROR_MESSAGES: Record<string, string> = {
  domain_not_allowed: 'Your email domain is not authorized. Contact your admin.',
  consent_denied: 'Google sign-in was cancelled.',
  invalid_state: 'Login session expired. Please try again.',
  email_not_verified: 'Your Google email is not verified.',
  auth_failed: 'Authentication failed. Please try again.',
}

export function Login() {
  const [params] = useSearchParams()
  const navigate = useNavigate()
  const [checking, setChecking] = useState(true)
  const error = params.get('error')

  useEffect(() => {
    fetch('/_ui/api/auth/me', { credentials: 'include' })
      .then(res => {
        if (res.ok) {
          navigate('/', { replace: true })
        } else {
          setChecking(false)
        }
      })
      .catch(() => setChecking(false))
  }, [navigate])

  if (checking) {
    return (
      <div className="auth-overlay">
        <div role="status" aria-label="Loading" style={{ color: 'var(--text-tertiary)', fontSize: '0.8rem', fontFamily: 'var(--font-mono)' }}>
          Loading...
        </div>
      </div>
    )
  }

  function handleGoogleLogin() {
    window.location.href = '/_ui/api/auth/google'
  }

  return (
    <div className="auth-overlay">
      <div className="auth-card">
        <div className="auth-logo">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--bg)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M6 2v20"/><path d="M18 2v20"/><path d="M6 2h12"/><path d="M6 22h12"/><path d="M12 8l2.5 2.5-2.5 2.5-2.5-2.5z"/>
          </svg>
        </div>
        <h2><span aria-hidden="true">{'> '}</span>HARBANGAN<span className="cursor" aria-hidden="true" /></h2>
        <p>sign in to continue</p>

        {error && (
          <div className="login-error" role="alert" aria-live="assertive">
            {ERROR_MESSAGES[error] || 'Authentication failed. Please try again.'}
          </div>
        )}

        <button className="auth-submit" type="button" onClick={handleGoogleLogin}>
          $ sign in with google
        </button>
      </div>
    </div>
  )
}
