import { useState, useEffect } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { apiFetch, apiPut, apiDelete } from "../lib/api";
import type { UserDetailResponse } from "../lib/api";
import { useToast } from "../components/useToast";

export function UserDetail() {
  const { userId } = useParams<{ userId: string }>();
  const navigate = useNavigate();
  const { showToast } = useToast();
  const [data, setData] = useState<UserDetailResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  useEffect(() => {
    apiFetch<UserDetailResponse>(`/users/${userId}`)
      .then(setData)
      .catch(() => showToast("Failed to load user", "error"))
      .finally(() => setLoading(false));
  }, [userId, showToast]);

  async function handleRoleToggle() {
    if (!data) return;
    const newRole = data.user.role === "admin" ? "user" : "admin";
    try {
      await apiPut(`/users/${userId}/role`, { role: newRole });
      setData({ ...data, user: { ...data.user, role: newRole } });
      showToast(`Role changed to ${newRole}`, "success");
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to update role",
        "error",
      );
    }
  }

  function handleDelete() {
    if (!data) return;
    setConfirmState({
      action: async () => {
        try {
          await apiDelete(`/users/${userId}`);
          showToast(`${data.user.email} removed`, "success");
          navigate("/admin");
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to remove user",
            "error",
          );
        }
      },
      title: "Remove user",
      message: `Remove ${data.user.email}? This cannot be undone.`,
    });
  }

  if (loading) {
    return (
      <div
        className="skeleton skeleton-block"
        role="status"
        aria-label="Loading user"
      />
    );
  }

  if (!data) {
    return <div className="empty-state">User not found</div>;
  }

  const { user, api_keys, kiro_status } = data;

  return (
    <>
      <div style={{ marginBottom: 16 }}>
        <Link
          to="/admin"
          style={{ color: "var(--text-secondary)", fontSize: "0.72rem" }}
        >
          {"< back to admin"}
        </Link>
      </div>

      <h2 className="section-header">USER DETAIL</h2>
      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">{"> "}account</span>
          <button
            type="button"
            onClick={handleRoleToggle}
            className="role-badge"
            style={{
              background:
                user.role === "admin" ? "var(--green-dim)" : "var(--blue-dim)",
              color: user.role === "admin" ? "var(--green)" : "var(--blue)",
            }}
            title={`Click to ${user.role === "admin" ? "demote to user" : "promote to admin"}`}
          >
            {user.role}
          </button>
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "4px 0",
          }}
        >
          {user.picture_url && (
            <img
              src={user.picture_url}
              alt=""
              style={{
                width: 32,
                height: 32,
                borderRadius: "var(--radius)",
                opacity: 0.85,
              }}
            />
          )}
          <div>
            <div
              style={{
                fontSize: "0.82rem",
                color: "var(--text)",
                fontWeight: 500,
              }}
            >
              {user.name}
            </div>
            <div style={{ fontSize: "0.72rem", color: "var(--text-tertiary)" }}>
              {user.email}
            </div>
          </div>
        </div>
        <div
          style={{
            fontSize: "0.68rem",
            color: "var(--text-secondary)",
            marginTop: 8,
          }}
        >
          joined {new Date(user.created_at).toLocaleDateString()}
          {user.last_login && (
            <> · last login {new Date(user.last_login).toLocaleDateString()}</>
          )}
        </div>
      </div>

      <h2 className="section-header">KIRO TOKEN</h2>
      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">{"> "}status</span>
        </div>
        <div style={{ fontSize: "0.78rem", padding: "4px 0" }}>
          {kiro_status.has_token ? (
            kiro_status.expired ? (
              <span style={{ color: "var(--red)" }}>expired</span>
            ) : (
              <span style={{ color: "var(--green)" }}>connected</span>
            )
          ) : (
            <span style={{ color: "var(--text-secondary)" }}>
              not configured
            </span>
          )}
        </div>
      </div>

      <h2 className="section-header">API KEYS</h2>
      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">{"> "}keys</span>
          <span className="card-subtitle">{api_keys.length} total</span>
        </div>
        {api_keys.length === 0 ? (
          <div className="empty-state">No API keys</div>
        ) : (
          <table className="data-table">
            <caption className="sr-only">API Keys</caption>
            <thead>
              <tr>
                <th scope="col">prefix</th>
                <th scope="col">label</th>
                <th scope="col">created</th>
                <th scope="col">last used</th>
              </tr>
            </thead>
            <tbody>
              {api_keys.map((k) => (
                <tr key={k.id}>
                  <td
                    style={{
                      fontFamily: "var(--font-mono)",
                      fontSize: "0.72rem",
                    }}
                  >
                    {k.key_prefix}...
                  </td>
                  <td>{k.label}</td>
                  <td style={{ color: "var(--text-secondary)" }}>
                    {new Date(k.created_at).toLocaleDateString()}
                  </td>
                  <td style={{ color: "var(--text-secondary)" }}>
                    {k.last_used
                      ? new Date(k.last_used).toLocaleDateString()
                      : "\u2014"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <button
        type="button"
        className="device-code-cancel"
        onClick={handleDelete}
        style={{ color: "var(--red)", fontSize: "0.72rem" }}
      >
        remove user
      </button>
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
