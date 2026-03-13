import { NavLink } from 'react-router-dom'
import { useSession } from './SessionGate'
import { authHeaders } from '../lib/auth'
import { ThemeToggle } from './ThemeToggle'

interface SidebarProps {
  open?: boolean
  onClose?: () => void
}

export function Sidebar({ open, onClose }: SidebarProps) {
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
    <nav className={`sidebar${open ? ' open' : ''}`} aria-label="Main navigation" onClick={e => e.stopPropagation()}>
      <div className="sidebar-brand">
        <h1 aria-label="Harbangan"><span aria-hidden="true">{'  ╔◈╗   ╔◈╗\n  ║ ║   ║ ║\n  ╠═╩═◈═╩═╣\n  ║ │   │ ║\n  ╚═╧═◈═╧═╝\n HARBANGAN'}</span></h1>
        <div className="version">v1.0.8</div>
      </div>
      <div className="sidebar-nav">
        <NavLink to="/profile" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
          <span className="nav-cursor">{'>'}</span> profile
        </NavLink>
        <NavLink to="/providers" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
          <span className="nav-cursor">{'>'}</span> providers
        </NavLink>
        {user.role === 'admin' && (
          <>
            <NavLink to="/config" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
              <span className="nav-cursor">{'>'}</span> config
            </NavLink>
            <NavLink to="/guardrails" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
              <span className="nav-cursor">{'>'}</span> guardrails
            </NavLink>
            <NavLink to="/mcp" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
              <span className="nav-cursor">{'>'}</span> mcp servers
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
        <ThemeToggle />
        <button className="btn-logout" onClick={handleLogout} title="Sign out">
          $ logout
        </button>
      </div>
    </nav>
  )
}
