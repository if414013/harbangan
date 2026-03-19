import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { ConfirmDialog } from "./ConfirmDialog";
import { apiFetch, apiPut, apiDelete, adminResetPassword } from "../lib/api";
import type { User } from "../lib/api";
import { useToast } from "./useToast";

export function UserTable() {
  const { showToast } = useToast();
  const [users, setUsers] = useState<User[]>([]);
  const [loading, setLoading] = useState(true);
  const [resetUserId, setResetUserId] = useState<string | null>(null);
  const [resetPassword, setResetPassword] = useState("");
  const [resetting, setResetting] = useState(false);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  function loadUsers() {
    apiFetch<{ users: User[] }>("/users")
      .then((data) => {
        setUsers(data.users);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }

  useEffect(() => {
    loadUsers();
  }, []);

  async function handleRoleChange(user: User) {
    const newRole = user.role === "admin" ? "user" : "admin";
    try {
      await apiPut(`/users/${user.id}/role`, { role: newRole });
      showToast(`${user.email} is now ${newRole}`, "success");
      loadUsers();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to update role",
        "error",
      );
    }
  }

  function handleDelete(user: User) {
    setConfirmState({
      action: async () => {
        try {
          await apiDelete(`/users/${user.id}`);
          showToast(`${user.email} removed`, "success");
          loadUsers();
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to remove user",
            "error",
          );
        }
      },
      title: "Remove user",
      message: `Remove ${user.email}? This cannot be undone.`,
    });
  }

  async function handleResetPassword() {
    if (!resetUserId || !resetPassword) return;
    setResetting(true);
    try {
      await adminResetPassword(resetUserId, resetPassword);
      showToast("Password reset successfully", "success");
      setResetUserId(null);
      setResetPassword("");
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to reset password",
        "error",
      );
    } finally {
      setResetting(false);
    }
  }

  if (loading) {
    return (
      <div
        className="skeleton skeleton-block"
        role="status"
        aria-label="Loading users"
      />
    );
  }

  return (
    <>
      <div className="card">
        <div className="card-header">
          <span className="card-title">{"> "}users</span>
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
                <th scope="col">auth</th>
                <th scope="col">role</th>
                <th scope="col">last login</th>
                <th scope="col">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id}>
                  <td>
                    <Link
                      to={`/admin/users/${u.id}`}
                      style={{ color: "var(--text)", textDecoration: "none" }}
                    >
                      {u.picture_url && (
                        <img
                          src={u.picture_url}
                          alt=""
                          style={{
                            width: 18,
                            height: 18,
                            borderRadius: "var(--radius-sm)",
                            marginRight: 8,
                            verticalAlign: "middle",
                            opacity: 0.8,
                          }}
                        />
                      )}
                      {u.email}
                    </Link>
                  </td>
                  <td style={{ color: "var(--text-secondary)" }}>{u.name}</td>
                  <td>
                    <span
                      className="auth-method-badge"
                      style={{
                        background:
                          u.auth_method === "google"
                            ? "var(--blue-dim)"
                            : "var(--yellow-dim)",
                        color:
                          u.auth_method === "google"
                            ? "var(--blue)"
                            : "var(--yellow)",
                      }}
                    >
                      {u.auth_method === "google" ? "google" : "password"}
                    </span>
                  </td>
                  <td>
                    <button
                      type="button"
                      onClick={() => handleRoleChange(u)}
                      className="role-badge"
                      style={{
                        background:
                          u.role === "admin"
                            ? "var(--green-dim)"
                            : "var(--blue-dim)",
                        color:
                          u.role === "admin" ? "var(--green)" : "var(--blue)",
                      }}
                      title={`Click to ${u.role === "admin" ? "demote to user" : "promote to admin"}`}
                    >
                      {u.role}
                    </button>
                  </td>
                  <td style={{ color: "var(--text-secondary)" }}>
                    {u.last_login
                      ? new Date(u.last_login).toLocaleDateString()
                      : "\u2014"}
                  </td>
                  <td>
                    <span style={{ display: "inline-flex", gap: 8 }}>
                      {u.auth_method === "password" && (
                        <button
                          className="device-code-cancel"
                          type="button"
                          onClick={() => {
                            setResetUserId(u.id);
                            setResetPassword("");
                          }}
                          style={{ color: "var(--yellow)" }}
                          aria-label={`Reset password for ${u.email}`}
                        >
                          reset pw
                        </button>
                      )}
                      <button
                        className="device-code-cancel"
                        type="button"
                        onClick={() => handleDelete(u)}
                        style={{ color: "var(--red)" }}
                        aria-label={`Remove user ${u.email}`}
                      >
                        remove
                      </button>
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {resetUserId && (
        <div className="modal-overlay" onClick={() => setResetUserId(null)}>
          <div className="modal-box" onClick={(e) => e.stopPropagation()}>
            <h3>{"> "}Reset Password</h3>
            <p>Enter the new password for this user.</p>
            <input
              className="auth-input"
              type="password"
              placeholder="new password (min 8 chars)"
              value={resetPassword}
              onChange={(e) => setResetPassword(e.target.value)}
              minLength={8}
              autoFocus
            />
            <div className="modal-actions">
              <button type="button" onClick={() => setResetUserId(null)}>
                cancel
              </button>
              <button
                type="button"
                className="modal-confirm"
                onClick={handleResetPassword}
                disabled={resetting || resetPassword.length < 8}
              >
                {resetting ? "resetting..." : "reset password"}
              </button>
            </div>
          </div>
        </div>
      )}
      {confirmState && (
        <ConfirmDialog
          title={confirmState.title}
          message={confirmState.message}
          confirmLabel="Remove"
          variant="danger"
          onConfirm={() => {
            confirmState.action();
            setConfirmState(null);
          }}
          onCancel={() => setConfirmState(null)}
        />
      )}
    </>
  );
}
