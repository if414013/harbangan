import { useState, useEffect } from 'react'
import { apiFetch, apiPut, apiDelete } from '../lib/api'
import type { User } from '../lib/api'
import { useToast } from './Toast'

export function UserTable() {
  const { showToast } = useToast()
  const [users, setUsers] = useState<User[]>([])
  const [loading, setLoading] = useState(true)

  function loadUsers() {
    apiFetch<{ users: User[] }>('/users')
      .then(data => { setUsers(data.users); setLoading(false) })
      .catch(() => setLoading(false))
  }

  useEffect(() => { loadUsers() }, [])

  async function handleRoleChange(user: User) {
    const newRole = user.role === 'admin' ? 'user' : 'admin'
    try {
      await apiPut(`/users/${user.id}/role`, { role: newRole })
      showToast(`${user.email} is now ${newRole}`, 'success')
      loadUsers()
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : 'Failed to update role',
        'error',
      )
    }
  }

  async function handleDelete(user: User) {
    if (!confirm(`Remove ${user.email}? This cannot be undone.`)) return
    try {
      await apiDelete(`/users/${user.id}`)
      showToast(`${user.email} removed`, 'success')
      loadUsers()
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : 'Failed to remove user',
        'error',
      )
    }
  }

  if (loading) {
    return <div className="skeleton skeleton-block" role="status" aria-label="Loading users" />
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{'> '}users</span>
        <span className="card-subtitle">{users.length} total</span>
      </div>
      {users.length === 0 ? (
        <div className="empty-state">No users yet</div>
      ) : (
        <table className="data-table">
          <caption className="sr-only">Users</caption>
          <thead>
            <tr>
              <th scope="col">email</th>
              <th scope="col">name</th>
              <th scope="col">role</th>
              <th scope="col">last login</th>
              <th scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {users.map(u => (
              <tr key={u.id}>
                <td>
                  {u.picture_url && (
                    <img
                      src={u.picture_url}
                      alt=""
                      style={{
                        width: 18,
                        height: 18,
                        borderRadius: 'var(--radius-sm)',
                        marginRight: 8,
                        verticalAlign: 'middle',
                        opacity: 0.8,
                      }}
                    />
                  )}
                  {u.email}
                </td>
                <td style={{ color: 'var(--text-secondary)' }}>{u.name}</td>
                <td>
                  <button
                    type="button"
                    onClick={() => handleRoleChange(u)}
                    className="role-badge"
                    style={{
                      background: u.role === 'admin' ? 'var(--green-dim)' : 'var(--blue-dim)',
                      color: u.role === 'admin' ? 'var(--green)' : 'var(--blue)',
                    }}
                    title={`Click to ${u.role === 'admin' ? 'demote to user' : 'promote to admin'}`}
                  >
                    {u.role}
                  </button>
                </td>
                <td style={{ color: 'var(--text-secondary)' }}>
                  {u.last_login ? new Date(u.last_login).toLocaleDateString() : '\u2014'}
                </td>
                <td>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => handleDelete(u)}
                    style={{ color: 'var(--red)' }}
                    aria-label={`Remove user ${u.email}`}
                  >
                    remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
