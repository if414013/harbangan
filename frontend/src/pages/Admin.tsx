import { useState, useEffect } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { UserTable } from "../components/UserTable";
import { DomainManager } from "../components/DomainManager";
import { PageHeader } from "../components/PageHeader";
import { useSession } from "../components/SessionGate";
import { useToast } from "../components/useToast";
import {
  adminCreateUser,
  getAdminPoolAccounts,
  addAdminPoolAccount,
  deleteAdminPoolAccount,
  toggleAdminPoolAccount,
} from "../lib/api";
import type { AdminPoolAccount } from "../lib/api";

const POOL_PROVIDERS = [
  "anthropic",
  "openai_codex",
  "kiro",
  "copilot",
  "qwen",
] as const;

function ProviderPool() {
  const { showToast } = useToast();
  const [accounts, setAccounts] = useState<AdminPoolAccount[]>([]);
  const [loading, setLoading] = useState(true);

  const [provider, setProvider] = useState("anthropic");
  const [label, setLabel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [adding, setAdding] = useState(false);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  function load() {
    getAdminPoolAccounts()
      .then((data) => {
        setAccounts(data.accounts);
        setLoading(false);
      })
      .catch(() => {
        setLoading(false);
      });
  }

  useEffect(() => {
    load();
  }, []);

  async function handleAdd(e: React.FormEvent) {
    e.preventDefault();
    if (!label || !apiKey) return;
    setAdding(true);
    try {
      await addAdminPoolAccount({
        provider_id: provider,
        account_label: label,
        api_key: apiKey,
        base_url: baseUrl || undefined,
      });
      showToast(`Pool account "${label}" added`, "success");
      setLabel("");
      setApiKey("");
      setBaseUrl("");
      load();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to add account",
        "error",
      );
    } finally {
      setAdding(false);
    }
  }

  async function handleToggle(id: string, enabled: boolean) {
    try {
      await toggleAdminPoolAccount(id, enabled);
      setAccounts((prev) =>
        prev.map((a) => (a.id === id ? { ...a, enabled } : a)),
      );
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to toggle account",
        "error",
      );
    }
  }

  function handleDelete(id: string, accountLabel: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteAdminPoolAccount(id);
          showToast(`Pool account "${accountLabel}" deleted`, "success");
          setAccounts((prev) => prev.filter((a) => a.id !== id));
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to delete account",
            "error",
          );
        }
      },
      title: "Delete pool account",
      message: `Delete pool account "${accountLabel}"?`,
    });
  }

  if (loading) {
    return (
      <div
        className="skeleton skeleton-block"
        role="status"
        aria-label="Loading pool accounts"
      />
    );
  }

  return (
    <>
      <div className="card mb-24">
        <form onSubmit={handleAdd}>
          <div className="create-user-form">
            <div className="create-user-row">
              <select
                className="config-input"
                value={provider}
                onChange={(e) => setProvider(e.target.value)}
              >
                {POOL_PROVIDERS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
              <input
                className="config-input"
                type="text"
                placeholder="account label"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                required
              />
            </div>
            <div className="create-user-row">
              <input
                className="config-input"
                type="password"
                placeholder="API key"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                required
              />
              <input
                className="config-input"
                type="text"
                placeholder="base URL (optional)"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
              />
            </div>
            <div>
              <button type="submit" className="btn-save" disabled={adding}>
                {adding ? "Adding..." : "Add Pool Account"}
              </button>
            </div>
          </div>
        </form>
      </div>

      {accounts.length === 0 ? (
        <div className="card">
          <div className="empty-state">No pool accounts configured.</div>
        </div>
      ) : (
        <table className="data-table">
          <caption className="sr-only">Admin provider pool accounts</caption>
          <thead>
            <tr>
              <th scope="col">status</th>
              <th scope="col">provider</th>
              <th scope="col">label</th>
              <th scope="col">key prefix</th>
              <th scope="col">base url</th>
              <th scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {accounts.map((a) => (
              <tr key={a.id}>
                <td>
                  <button
                    type="button"
                    className="role-badge"
                    onClick={() => handleToggle(a.id, !a.enabled)}
                    aria-label={`Toggle ${a.account_label} ${a.enabled ? "off" : "on"}`}
                    style={{
                      background: a.enabled
                        ? "var(--green-dim)"
                        : "var(--red-dim)",
                      color: a.enabled ? "var(--green)" : "var(--red)",
                    }}
                  >
                    {a.enabled ? "on" : "off"}
                  </button>
                </td>
                <td>{a.provider_id}</td>
                <td>{a.account_label}</td>
                <td style={{ color: "var(--text-tertiary)" }}>
                  {a.key_prefix || "—"}
                </td>
                <td style={{ color: "var(--text-tertiary)" }}>
                  {a.base_url || "—"}
                </td>
                <td>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => handleDelete(a.id, a.account_label)}
                    style={{ color: "var(--red)" }}
                  >
                    delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {confirmState && (
        <ConfirmDialog
          title={confirmState.title}
          message={confirmState.message}
          confirmLabel="Delete"
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

export function Admin() {
  const { setupComplete } = useSession();
  const { showToast } = useToast();
  const [newEmail, setNewEmail] = useState("");
  const [newName, setNewName] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [newRole, setNewRole] = useState<"admin" | "user">("user");
  const [creating, setCreating] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);

  async function handleCreateUser(e: React.FormEvent) {
    e.preventDefault();
    if (!newEmail || !newName || !newPassword) return;
    setCreating(true);
    try {
      await adminCreateUser(newEmail, newName, newPassword, newRole);
      showToast(`User ${newEmail} created`, "success");
      setNewEmail("");
      setNewName("");
      setNewPassword("");
      setNewRole("user");
      setRefreshKey((k) => k + 1);
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to create user",
        "error",
      );
    } finally {
      setCreating(false);
    }
  }

  return (
    <>
      <PageHeader
        title="administration"
        description="Manage users, provider pool accounts, and domain access."
      />
      {!setupComplete && (
        <div className="setup-banner">
          <div className="setup-banner-icon">!</div>
          <div>
            <strong>Welcome, admin!</strong> Your gateway is almost ready. Add
            your organization's domain below to restrict who can sign in. Leave
            empty to allow any Google account.
          </div>
        </div>
      )}

      <h2 className="section-header">PROVIDER POOL</h2>
      <div className="mb-24">
        <ProviderPool />
      </div>

      <h2 className="section-header">DOMAIN ALLOWLIST</h2>
      <div className="mb-24">
        <DomainManager />
      </div>

      <h2 className="section-header">CREATE PASSWORD USER</h2>
      <div className="card mb-24">
        <form onSubmit={handleCreateUser}>
          <div className="create-user-form">
            <div className="create-user-row">
              <input
                className="config-input"
                type="email"
                placeholder="email"
                value={newEmail}
                onChange={(e) => setNewEmail(e.target.value)}
                required
              />
              <input
                className="config-input"
                type="text"
                placeholder="name"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                required
              />
            </div>
            <div className="create-user-row">
              <input
                className="config-input"
                type="password"
                placeholder="password (min 8 chars)"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                minLength={8}
                required
              />
              <select
                className="config-input"
                value={newRole}
                onChange={(e) => setNewRole(e.target.value as "admin" | "user")}
              >
                <option value="user">user</option>
                <option value="admin">admin</option>
              </select>
            </div>
            <div>
              <button type="submit" className="btn-save" disabled={creating}>
                {creating ? "Creating..." : "Create User"}
              </button>
            </div>
          </div>
        </form>
      </div>

      <h2 className="section-header">USER MANAGEMENT</h2>
      <UserTable key={refreshKey} />
    </>
  );
}
