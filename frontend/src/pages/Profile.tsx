import { useNavigate } from 'react-router-dom'
import { ApiKeyManager } from '../components/ApiKeyManager'
import { useSession } from '../components/SessionGate'

export function Profile() {
  const { user } = useSession()
  const navigate = useNavigate()

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

      {user.auth_method === 'password' && (
        <>
          <h2 className="section-header">SECURITY</h2>
          <div className="card">
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <span style={{ fontSize: '0.72rem', color: 'var(--text-secondary)' }}>
                  2FA: {user.totp_enabled ? (
                    <span style={{ color: 'var(--green)' }}>ENABLED</span>
                  ) : (
                    <span style={{ color: 'var(--red)' }}>NOT SET UP</span>
                  )}
                </span>
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button className="btn-save" type="button" onClick={() => navigate('/change-password')}>
                  $ change password
                </button>
                <button className="btn-save" type="button" onClick={() => navigate('/setup-2fa')}>
                  $ reset 2fa
                </button>
              </div>
            </div>
          </div>
        </>
      )}
    </>
  )
}
