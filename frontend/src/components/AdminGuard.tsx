import type { ReactNode } from 'react'
import { Navigate } from 'react-router-dom'
import { useSession } from './SessionGate'

interface AdminGuardProps {
  children: ReactNode
}

export function AdminGuard({ children }: AdminGuardProps) {
  const { user } = useSession()
  if (user.role !== 'admin') return <Navigate to="/" replace />
  return <>{children}</>
}
