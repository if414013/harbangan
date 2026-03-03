import { KiroSetup } from '../components/KiroSetup'
import { ApiKeyManager } from '../components/ApiKeyManager'
import { useSession } from '../components/SessionGate'

export function Profile() {
  const { user } = useSession()

  return (
    <>
      <h2 className="section-header">PROFILE</h2>
      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">{'> '}account</span>
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

      <h2 className="section-header">KIRO TOKEN</h2>
      <div className="mb-24">
        <KiroSetup />
      </div>

      <h2 className="section-header">API KEYS</h2>
      <ApiKeyManager />
    </>
  )
}
