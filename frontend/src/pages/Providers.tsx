import { useState, useEffect, useRef } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { KiroSetup } from "../components/KiroSetup";
import { CopilotSetup } from "../components/CopilotSetup";
import { PageHeader } from "../components/PageHeader";
import { QwenSetup } from "../components/QwenSetup";
import { useToast } from "../components/useToast";
import {
  getProvidersStatus,
  getProviderConnectUrl,
  disconnectProvider,
  getRegistryModels,
  updateModelEnabled,
  deleteRegistryModel,
  populateModels,
  getUserProviderAccounts,
  deleteUserProviderAccount,
  getProviderRateLimits,
} from "../lib/api";
import type {
  ProvidersStatusResponse,
  RegistryModel,
  UserProviderAccount,
  RateLimitInfo,
} from "../lib/api";

const PROVIDERS = ["anthropic", "openai_codex"] as const;

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  openai_codex: "OpenAI Codex",
};

function providerDisplayName(id: string): string {
  return PROVIDER_DISPLAY_NAMES[id] ?? id.charAt(0).toUpperCase() + id.slice(1);
}

const RELAY_TIMEOUT_MS = 10 * 60 * 1000;

interface RelayModalProps {
  provider: string;
  relayScriptUrl: string;
  onConnected: () => void;
  onClose: () => void;
}

function RelayModal({
  provider,
  relayScriptUrl,
  onConnected,
  onClose,
}: RelayModalProps) {
  const [copied, setCopied] = useState(false);
  const [timedOut, setTimedOut] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);

  const curlCommand = `curl -fsSL '${relayScriptUrl}' | sh`;

  useEffect(() => {
    mountedRef.current = true;

    pollRef.current = setInterval(async () => {
      if (!mountedRef.current) return;
      try {
        const status = await getProvidersStatus();
        if (!mountedRef.current) return;
        const p = status.providers[provider];
        if (p?.connected) {
          onConnected();
        }
      } catch {
        // ignore poll errors
      }
    }, 2000);

    timeoutRef.current = setTimeout(() => {
      if (!mountedRef.current) return;
      setTimedOut(true);
      if (pollRef.current) clearInterval(pollRef.current);
    }, RELAY_TIMEOUT_MS);

    return () => {
      mountedRef.current = false;
      if (pollRef.current) clearInterval(pollRef.current);
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, [provider, onConnected]);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(curlCommand);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-box relay-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <h3>connect {provider}</h3>
        {timedOut ? (
          <>
            <p className="relay-timeout">
              Connection timed out. Click connect to try again.
            </p>
            <div className="modal-actions">
              <button type="button" onClick={onClose}>
                $ close
              </button>
            </div>
          </>
        ) : (
          <>
            <p>Run this in your terminal:</p>
            <div className="relay-command-wrap">
              <code className="relay-command">{curlCommand}</code>
              <button
                type="button"
                className="relay-copy-btn"
                onClick={handleCopy}
              >
                {copied ? "[copied]" : "[copy]"}
              </button>
            </div>
            <div className="device-code-polling">
              <span className="cursor" />
              waiting for authorization...
            </div>
            <div className="modal-actions">
              <button type="button" onClick={onClose}>
                $ cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

interface ProviderCardProps {
  provider: string;
  connected: boolean;
  email?: string;
  accounts: UserProviderAccount[];
  rateLimits: RateLimitInfo[];
  onRefresh: () => void;
}

function ProviderCard({
  provider,
  connected,
  email,
  accounts,
  rateLimits,
  onRefresh,
}: ProviderCardProps) {
  const { showToast } = useToast();
  const [connecting, setConnecting] = useState(false);
  const [relayUrl, setRelayUrl] = useState<string | null>(null);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  async function handleConnect() {
    setConnecting(true);
    try {
      const result = await getProviderConnectUrl(provider);
      setRelayUrl(result.relay_script_url);
    } catch (err) {
      showToast(
        "Failed to start connect: " +
          (err instanceof Error ? err.message : "Unknown error"),
        "error",
      );
    } finally {
      setConnecting(false);
    }
  }

  async function handleDisconnect() {
    try {
      await disconnectProvider(provider);
      showToast(`${provider} disconnected`, "success");
      onRefresh();
    } catch (err) {
      showToast(
        "Failed to disconnect: " +
          (err instanceof Error ? err.message : "Unknown error"),
        "error",
      );
    }
  }

  function handleDeleteAccount(label: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteUserProviderAccount(provider, label);
          showToast(`Account "${label}" removed`, "success");
          onRefresh();
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to remove account",
            "error",
          );
        }
      },
      title: "Remove account",
      message: `Remove account "${label}" from ${providerDisplayName(provider)}?`,
    });
  }

  function handleConnected() {
    setRelayUrl(null);
    showToast(`${provider} connected`, "success");
    onRefresh();
  }

  function getRateLimit(label: string): RateLimitInfo | undefined {
    return rateLimits.find(
      (r) => r.provider_id === provider && r.account_label === label,
    );
  }

  return (
    <>
      <div className="card provider-card">
        <div className="card-header">
          <span className="card-title">
            {"> "}
            {providerDisplayName(provider)}
          </span>
          {connected ? (
            <span className="tag-ok">CONNECTED</span>
          ) : (
            <span className="tag-err">NOT CONNECTED</span>
          )}
        </div>
        {connected && email && accounts.length === 0 && (
          <div className="provider-email">{email}</div>
        )}

        {accounts.length > 0 && (
          <div className="account-list">
            {accounts.map((acct) => {
              const rl = getRateLimit(acct.account_label);
              const isLimited = rl?.limited_until != null;
              return (
                <div key={acct.account_label} className="account-row">
                  <div className="account-row-info">
                    <span className="account-label">{acct.account_label}</span>
                    {acct.email && (
                      <span className="account-email">{acct.email}</span>
                    )}
                    {isLimited && (
                      <span className="tag-warn">RATE LIMITED</span>
                    )}
                    {rl && rl.requests_remaining != null && !isLimited && (
                      <span className="account-rate">
                        {rl.requests_remaining} req left
                      </span>
                    )}
                  </div>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => handleDeleteAccount(acct.account_label)}
                    style={{ color: "var(--red)" }}
                  >
                    remove
                  </button>
                </div>
              );
            })}
          </div>
        )}

        <div className="kiro-actions">
          {connected ? (
            <>
              <button
                className="btn-save"
                type="button"
                onClick={handleConnect}
                disabled={connecting}
              >
                {connecting ? "..." : "$ connect another"}
              </button>
              <button
                className="device-code-cancel"
                type="button"
                onClick={handleDisconnect}
              >
                $ disconnect all
              </button>
            </>
          ) : (
            <button
              className="btn-save"
              type="button"
              onClick={handleConnect}
              disabled={connecting}
            >
              {connecting ? "..." : "$ connect"}
            </button>
          )}
        </div>
      </div>
      {relayUrl && (
        <RelayModal
          provider={provider}
          relayScriptUrl={relayUrl}
          onConnected={handleConnected}
          onClose={() => setRelayUrl(null)}
        />
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

interface TreeNodeProps {
  label: string;
  last?: boolean;
  children: React.ReactNode;
}

function TreeNode({ label, last, children }: TreeNodeProps) {
  const [open, setOpen] = useState(false);

  return (
    <div className={`tree-node${last ? " tree-node-last" : ""}`}>
      <button
        type="button"
        className="tree-node-toggle"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="tree-branch">{last ? "└" : "├"}</span>
        <span className="tree-arrow">{open ? "▼" : "▶"}</span>
        <span className="tree-label">{label}</span>
      </button>
      {open && (
        <div className="tree-node-content">
          <div
            className={`tree-node-line${last ? " tree-node-line-hidden" : ""}`}
          />
          <div className="tree-node-body">{children}</div>
        </div>
      )}
    </div>
  );
}

interface ProviderGroup {
  providerId: string;
  models: RegistryModel[];
}

function groupByProvider(models: RegistryModel[]): ProviderGroup[] {
  const map = new Map<string, RegistryModel[]>();
  for (const m of models) {
    const list = map.get(m.provider_id) ?? [];
    list.push(m);
    map.set(m.provider_id, list);
  }
  return Array.from(map.entries())
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([providerId, models]) => ({ providerId, models }));
}

function ProviderSection({
  group,
  onToggle,
  onDelete,
  onPopulate,
}: {
  group: ProviderGroup;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
  onPopulate: (providerId: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const enabledCount = group.models.filter((m) => m.enabled).length;

  function handleEnableAll() {
    for (const m of group.models) {
      if (!m.enabled) onToggle(m.id, true);
    }
  }

  function handleDisableAll() {
    for (const m of group.models) {
      if (m.enabled) onToggle(m.id, false);
    }
  }

  return (
    <div className={`config-group${collapsed ? " collapsed" : ""}`}>
      <div
        className="config-group-header"
        onClick={() => setCollapsed((c) => !c)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            setCollapsed((c) => !c);
          }
        }}
        tabIndex={0}
        role="button"
        aria-expanded={!collapsed}
      >
        <span>{group.providerId}</span>
        <span
          style={{
            marginLeft: "auto",
            fontSize: "0.62rem",
            color: "var(--text-tertiary)",
            fontWeight: 400,
          }}
        >
          {enabledCount}/{group.models.length} enabled
        </span>
      </div>
      <div className="config-group-body">
        <div style={{ padding: "8px 16px", display: "flex", gap: 8 }}>
          <button
            className="btn-reveal"
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onPopulate(group.providerId);
            }}
          >
            $ populate
          </button>
          <button
            className="btn-reveal"
            type="button"
            onClick={handleEnableAll}
          >
            enable all
          </button>
          <button
            className="btn-reveal"
            type="button"
            onClick={handleDisableAll}
          >
            disable all
          </button>
        </div>
        <table className="data-table">
          <caption className="sr-only">Models for {group.providerId}</caption>
          <thead>
            <tr>
              <th scope="col">enabled</th>
              <th scope="col">prefixed id</th>
              <th scope="col">display name</th>
              <th scope="col">context</th>
              <th scope="col">source</th>
              <th scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {group.models.map((m) => (
              <tr key={m.id}>
                <td>
                  <button
                    type="button"
                    className="role-badge"
                    onClick={() => onToggle(m.id, !m.enabled)}
                    aria-label={`Toggle ${m.prefixed_id} ${m.enabled ? "off" : "on"}`}
                    style={{
                      background: m.enabled
                        ? "var(--green-dim)"
                        : "var(--red-dim)",
                      color: m.enabled ? "var(--green)" : "var(--red)",
                    }}
                  >
                    {m.enabled ? "on" : "off"}
                  </button>
                </td>
                <td>{m.prefixed_id}</td>
                <td style={{ color: "var(--text-secondary)" }}>
                  {m.display_name}
                </td>
                <td style={{ color: "var(--text-tertiary)" }}>
                  {m.context_length.toLocaleString()}
                </td>
                <td>
                  <span className="source-badge">{m.source}</span>
                </td>
                <td>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => onDelete(m.id)}
                    aria-label={`Delete ${m.prefixed_id}`}
                    style={{ color: "var(--red)" }}
                  >
                    delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function Providers() {
  const { showToast } = useToast();
  const [providerStatus, setProviderStatus] =
    useState<ProvidersStatusResponse | null>(null);
  const [models, setModels] = useState<RegistryModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(true);
  const [populating, setPopulating] = useState(false);
  const [providerAccounts, setProviderAccounts] = useState<
    Record<string, UserProviderAccount[]>
  >({});
  const [rateLimits, setRateLimits] = useState<RateLimitInfo[]>([]);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  function loadProviders() {
    getProvidersStatus()
      .then(setProviderStatus)
      .catch(() => {});
  }

  function loadModels() {
    getRegistryModels()
      .then((data) => {
        setModels(data.models);
        setModelsLoading(false);
      })
      .catch(() => {
        setModelsLoading(false);
      });
  }

  function loadAccounts() {
    for (const p of PROVIDERS) {
      getUserProviderAccounts(p)
        .then((data) => {
          setProviderAccounts((prev) => ({ ...prev, [p]: data.accounts }));
        })
        .catch(() => {});
    }
  }

  function loadRateLimits() {
    getProviderRateLimits()
      .then((data) => setRateLimits(data.accounts))
      .catch(() => {});
  }

  function refreshAll() {
    loadProviders();
    loadAccounts();
    loadRateLimits();
  }

  useEffect(() => {
    loadProviders();
    loadModels();
    loadAccounts();
    loadRateLimits();
  }, []);

  async function handleToggle(id: string, enabled: boolean) {
    try {
      await updateModelEnabled(id, enabled);
      setModels((prev) =>
        prev.map((m) => (m.id === id ? { ...m, enabled } : m)),
      );
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to update model",
        "error",
      );
    }
  }

  function handleDelete(id: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteRegistryModel(id);
          showToast("Model deleted", "success");
          setModels((prev) => prev.filter((m) => m.id !== id));
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to delete model",
            "error",
          );
        }
      },
      title: "Delete model",
      message: "Delete this model from the registry?",
    });
  }

  async function handlePopulate(providerId?: string) {
    setPopulating(true);
    try {
      const res = await populateModels(providerId);
      showToast(`Populated ${res.models_upserted} models`, "success");
      loadModels();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to populate models",
        "error",
      );
    } finally {
      setPopulating(false);
    }
  }

  const groups = groupByProvider(models);

  return (
    <>
      <PageHeader
        title="providers"
        description="Connect provider accounts and manage model access."
      />
      <div className="provider-tree">
        <TreeNode label="Kiro">
          <KiroSetup />
        </TreeNode>
        <TreeNode label="github copilot">
          <CopilotSetup />
        </TreeNode>
        <TreeNode label="qwen coder">
          <QwenSetup />
        </TreeNode>
        {PROVIDERS.map((p, i) => {
          const info = providerStatus?.providers[p];
          return (
            <TreeNode
              key={p}
              label={providerDisplayName(p)}
              last={i === PROVIDERS.length - 1}
            >
              <ProviderCard
                provider={p}
                connected={info?.connected ?? false}
                email={info?.email}
                accounts={providerAccounts[p] ?? []}
                rateLimits={rateLimits}
                onRefresh={refreshAll}
              />
            </TreeNode>
          );
        })}
      </div>

      <h2 className="section-header" style={{ marginTop: 32 }}>
        MODEL REGISTRY
      </h2>
      {modelsLoading ? (
        <div
          className="skeleton skeleton-block"
          role="status"
          aria-label="Loading models"
        />
      ) : (
        <>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              marginBottom: 16,
            }}
          >
            <button
              className="btn-save"
              type="button"
              onClick={() => handlePopulate()}
              disabled={populating}
            >
              {populating ? "populating..." : "$ populate all"}
            </button>
            <span className="card-subtitle">
              {models.length} models across {groups.length} providers
            </span>
          </div>
          {groups.length === 0 ? (
            <div className="card">
              <div className="empty-state">
                No models in registry. Click "populate all" to fetch models from
                connected providers.
              </div>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {groups.map((g) => (
                <ProviderSection
                  key={g.providerId}
                  group={g}
                  onToggle={handleToggle}
                  onDelete={handleDelete}
                  onPopulate={handlePopulate}
                />
              ))}
            </div>
          )}
        </>
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
