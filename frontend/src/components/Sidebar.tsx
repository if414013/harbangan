import { NavLink } from 'react-router-dom'
import { useSession } from './SessionGate'
import { authHeaders } from '../lib/auth'

interface SidebarProps {
  connected: boolean
  open?: boolean
  onClose?: () => void
}

export function Sidebar({ connected, open, onClose }: SidebarProps) {
  const { user } = useSession()

  async function handleLogout() {
    try {
      await fetch('/_ui/api/auth/logout', {
        method: 'POST',
        credentials: 'include',
        headers: authHeaders(),
      })
    } catch { /* ignore */ }
    window.location.href = '/_ui/login'
  }

  return (
    <nav className={`sidebar${open ? ' open' : ''}`} onClick={e => e.stopPropagation()}>
      <div className="sidebar-brand">
        <h1 aria-label="Kiro"><span aria-hidden="true">{'  _  ___\n | |/ (_)_ _ ___\n | \' <| | \'_/ _ \\\n |_|\\_\\_|_| \\___/'}</span></h1>
        <div className="version">gateway v1.0.8</div>
      </div>
      <div className="sidebar-nav">
        <NavLink to="/" end className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
          <span className="nav-cursor">{'>'}</span> dashboard
        </NavLink>
        <NavLink to="/profile" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
          <span className="nav-cursor">{'>'}</span> profile
        </NavLink>
        {user.role === 'admin' && (
          <>
            <NavLink to="/config" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
              <span className="nav-cursor">{'>'}</span> config
            </NavLink>
            <NavLink to="/admin" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
              <span className="nav-cursor">{'>'}</span> admin
            </NavLink>
          </>
        )}
      </div>
      <div className="sidebar-footer">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flex: 1, minWidth: 0 }}>
          {user.picture_url && (
            <img
              src={user.picture_url}
              alt=""
              style={{ width: 18, height: 18, borderRadius: 'var(--radius-sm)', opacity: 0.7, flexShrink: 0 }}
            />
          )}
          <span style={{ color: 'var(--text-tertiary)', fontSize: '0.62rem', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {user.email}
          </span>
        </div>
      </div>
      <div className="sidebar-footer-actions">
        {connected
          ? <span className="tag-ok">STREAM</span>
          : <span className="tag-err">STREAM</span>
        }
        <button className="btn-logout" onClick={handleLogout} title="Sign out">
          $ logout
        </button>
      </div>
    </nav>
  )
}
