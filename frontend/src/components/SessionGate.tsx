import { createContext, useContext, useState, useEffect } from 'react'
import type { ReactNode } from 'react'
import { Navigate } from 'react-router-dom'
import { checkSetupStatus } from '../lib/api'
import type { User } from '../lib/api'

interface SessionContextValue {
  user: User
  setupComplete: boolean
}

const SessionContext = createContext<SessionContextValue | null>(null)

// eslint-disable-next-line react-refresh/only-export-components
export function useSession(): SessionContextValue {
  const ctx = useContext(SessionContext)
  if (!ctx) throw new Error('useSession must be used within SessionGate')
  return ctx
}

interface SessionGateProps {
  children: ReactNode
}

export function SessionGate({ children }: SessionGateProps) {
  const [state, setState] = useState<{
    loading: boolean
    user: User | null
    setupComplete: boolean
  }>({ loading: true, user: null, setupComplete: false })

  useEffect(() => {
    Promise.all([
      fetch('/_ui/api/auth/me', { credentials: 'include' })
        .then(res => res.ok ? res.json() as Promise<User> : null)
        .catch(() => null),
      checkSetupStatus(),
    ]).then(([user, setupComplete]) => {
      setState({ loading: false, user, setupComplete })
    })
  }, [])

  if (state.loading) {
    return (
      <div className="auth-overlay">
        <div role="status" aria-label="Loading session" style={{ color: 'var(--text-tertiary)', fontSize: '0.8rem', fontFamily: 'var(--font-mono)' }}>
          Loading...
        </div>
      </div>
    )
  }

  if (!state.user) {
    return <Navigate to="/login" replace />
  }

  return (
    <SessionContext.Provider value={{ user: state.user, setupComplete: state.setupComplete }}>
      {children}
    </SessionContext.Provider>
  )
}
