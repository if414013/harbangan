import { NavLink } from 'react-router-dom'
import { setApiKey } from '../lib/auth'

const DashboardIcon = () => (
  <svg className="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/>
  </svg>
)

const ConfigIcon = () => (
  <svg className="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
  </svg>
)

interface SidebarProps {
  connected: boolean
  open?: boolean
  onClose?: () => void
}

export function Sidebar({ connected, open, onClose }: SidebarProps) {
  function handleLogout() {
    setApiKey('')
    sessionStorage.clear()
    window.location.reload()
  }

  return (
    <nav className={`sidebar${open ? ' open' : ''}`} onClick={e => e.stopPropagation()}>
      <div className="sidebar-brand">
        <h1>Kiro Gateway</h1>
        <div className="version">v1.0.8</div>
      </div>
      <div className="sidebar-nav">
        <NavLink to="/" end className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
          <DashboardIcon /> Dashboard
        </NavLink>
        <NavLink to="/config" className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`} onClick={onClose}>
          <ConfigIcon /> Configuration
        </NavLink>
      </div>
      <div className="sidebar-footer">
        <span className={`status-indicator ${connected ? 'connected' : 'disconnected'}`} />
        <span className="status-label">{connected ? 'Connected' : 'Disconnected'}</span>
        <button className="btn-logout" onClick={handleLogout} title="Disconnect and clear API key">
          Logout
        </button>
      </div>
    </nav>
  )
}
