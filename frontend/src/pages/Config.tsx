import { useState, useEffect, useRef, useMemo, type FormEvent } from "react";
import { PageHeader } from "../components/PageHeader";
import { apiFetch, apiPut } from "../lib/api";
import { useToast } from "../components/useToast";

interface HistoryEntry {
  key?: string;
  field?: string;
  old_value?: string | null;
  new_value?: string;
  changed_at?: string;
  timestamp?: string;
}

interface ConfigField {
  key: string;
  label: string;
  type: "text" | "number" | "checkbox" | "select" | "password";
  options?: string[];
  restart?: boolean;
}

const CONFIG_GROUPS: { title: string; icon: string; fields: ConfigField[] }[] =
  [
    {
      title: "Server",
      icon: "server",
      fields: [
        { key: "server_host", label: "Host", type: "text", restart: true },
        { key: "server_port", label: "Port", type: "number", restart: true },
      ],
    },
    {
      title: "Kiro Backend",
      icon: "globe",
      fields: [
        { key: "kiro_region", label: "Region", type: "text", restart: true },
      ],
    },
    {
      title: "Timeouts",
      icon: "clock",
      fields: [
        {
          key: "first_token_timeout",
          label: "First Token (s)",
          type: "number",
        },
        {
          key: "streaming_timeout",
          label: "Streaming (s)",
          type: "number",
          restart: true,
        },
        {
          key: "token_refresh_threshold",
          label: "Token Refresh (s)",
          type: "number",
          restart: true,
        },
      ],
    },
    {
      title: "Debug",
      icon: "edit",
      fields: [
        {
          key: "debug_mode",
          label: "Debug Mode",
          type: "select",
          options: ["off", "errors", "all"],
        },
        {
          key: "log_level",
          label: "Log Level",
          type: "select",
          options: ["trace", "debug", "info", "warn", "error"],
        },
      ],
    },
    {
      title: "Converter",
      icon: "shuffle",
      fields: [
        {
          key: "tool_description_max_length",
          label: "Tool Desc Max Length",
          type: "number",
        },
        {
          key: "fake_reasoning_enabled",
          label: "Fake Reasoning",
          type: "checkbox",
        },
        {
          key: "fake_reasoning_max_tokens",
          label: "Fake Reasoning Tokens",
          type: "number",
        },
      ],
    },
    {
      title: "HTTP Client",
      icon: "link",
      fields: [
        {
          key: "http_max_connections",
          label: "Max Connections",
          type: "number",
          restart: true,
        },
        {
          key: "http_connect_timeout",
          label: "Connect Timeout (s)",
          type: "number",
          restart: true,
        },
        {
          key: "http_request_timeout",
          label: "Request Timeout (s)",
          type: "number",
          restart: true,
        },
        {
          key: "http_max_retries",
          label: "Max Retries",
          type: "number",
          restart: true,
        },
      ],
    },
    {
      title: "Features",
      icon: "star",
      fields: [
        {
          key: "truncation_recovery",
          label: "Truncation Recovery",
          type: "checkbox",
        },
        { key: "guardrails_enabled", label: "Guardrails", type: "checkbox" },
      ],
    },
    {
      title: "Authentication",
      icon: "lock",
      fields: [
        { key: "auth_google_enabled", label: "Google SSO", type: "checkbox" },
        { key: "google_client_id", label: "Client ID", type: "text" },
        {
          key: "google_client_secret",
          label: "Client Secret",
          type: "password",
        },
        { key: "google_callback_url", label: "Callback URL", type: "text" },
        {
          key: "auth_password_enabled",
          label: "Password Auth",
          type: "checkbox",
        },
      ],
    },
  ];

const RESTART_KEYS = new Set(
  CONFIG_GROUPS.flatMap((g) =>
    g.fields.filter((f) => f.restart).map((f) => f.key),
  ),
);

const ICONS: Record<string, React.ReactNode> = {
  server: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <rect x="2" y="2" width="20" height="8" rx="2" />
      <rect x="2" y="14" width="20" height="8" rx="2" />
      <circle cx="6" cy="6" r="1" />
      <circle cx="6" cy="18" r="1" />
    </svg>
  ),
  lock: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <rect x="3" y="11" width="18" height="11" rx="2" />
      <path d="M7 11V7a5 5 0 0 1 10 0v4" />
    </svg>
  ),
  globe: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <circle cx="12" cy="12" r="10" />
      <path d="M2 12h20" />
      <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
    </svg>
  ),
  clock: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  ),
  link: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
      <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
    </svg>
  ),
  edit: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
    </svg>
  ),
  shuffle: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <polyline points="16 3 21 3 21 8" />
      <line x1="4" y1="20" x2="21" y2="3" />
      <polyline points="21 16 21 21 16 21" />
      <line x1="15" y1="15" x2="21" y2="21" />
      <line x1="4" y1="4" x2="9" y2="9" />
    </svg>
  ),
  star: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  ),
  key: (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4" />
    </svg>
  ),
};

export function Config() {
  const { showToast } = useToast();
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [revealPassword, setRevealPassword] = useState(false);
  const [loading, setLoading] = useState(true);
  const [dirty, setDirty] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [savedValues, setSavedValues] = useState<Record<string, unknown>>({});
  const savedSnapshot = useRef<string>("");

  function toggleGroup(name: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  }

  function loadHistory() {
    apiFetch<{ history: HistoryEntry[] }>("/config/history")
      .then((data) => setHistory(data.history || []))
      .catch(() => {});
  }

  useEffect(() => {
    apiFetch<{ setup_complete: boolean; config: Record<string, unknown> }>(
      "/config",
    )
      .then((data) => {
        setValues(data.config);
        setSavedValues(data.config);
        savedSnapshot.current = JSON.stringify(data.config);
        setLoading(false);
      })
      .catch((err) => {
        showToast("Failed to load config: " + err.message, "error");
        setLoading(false);
      });
    loadHistory();
  }, [showToast]);

  function handleChange(key: string, value: unknown) {
    setValues((prev) => {
      const next = { ...prev, [key]: value };
      setDirty(JSON.stringify(next) !== savedSnapshot.current);
      return next;
    });
  }

  function getChangedKeys(): string[] {
    try {
      const saved = JSON.parse(savedSnapshot.current) as Record<
        string,
        unknown
      >;
      return Object.keys(values).filter(
        (k) => JSON.stringify(values[k]) !== JSON.stringify(saved[k]),
      );
    } catch {
      return [];
    }
  }

  const changedKeysSet = useMemo(() => {
    return new Set(
      Object.keys(values).filter(
        (k) => JSON.stringify(values[k]) !== JSON.stringify(savedValues[k]),
      ),
    );
  }, [values, savedValues]);

  function getGroupSummary(fields: ConfigField[]): string {
    const total = fields.length;
    const modified = fields.filter((f) => changedKeysSet.has(f.key)).length;
    if (modified > 0) {
      return `${total} fields, ${modified} modified`;
    }
    return `${total} fields`;
  }

  function handleReset() {
    setValues(savedValues);
    setDirty(false);
  }

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const changed = getChangedKeys();
    if (changed.length === 0) return;
    const payload = Object.fromEntries(changed.map((k) => [k, values[k]]));
    const needsRestart = changed.some((k) => RESTART_KEYS.has(k));
    apiPut("/config", payload)
      .then(() => {
        savedSnapshot.current = JSON.stringify(values);
        setSavedValues(values);
        setDirty(false);
        if (needsRestart) {
          showToast(
            "Configuration saved — restart required for some changes to take effect",
            "success",
          );
        } else {
          showToast("Configuration saved — applied immediately", "success");
        }
        loadHistory();
      })
      .catch((err) => showToast("Failed to save: " + err.message, "error"));
  }

  if (loading) {
    return (
      <div className="config-layout">
        <div className="config-form-area">
          {[1, 2, 3, 4].map((i) => (
            <div key={i} className="skeleton skeleton-block" />
          ))}
        </div>
        <div className="skeleton skeleton-block" style={{ height: 200 }} />
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title="configuration"
        description="Runtime configuration for the gateway. Changes marked 'live' take effect immediately; 'restart' changes require a service restart."
      />
      <form onSubmit={handleSubmit}>
        <div className="config-layout">
          <div className="config-form-area">
            {CONFIG_GROUPS.map((group) => (
              <div
                key={group.title}
                className={`config-group${collapsed.has(group.title) ? " collapsed" : ""}`}
              >
                <h3
                  className="config-group-header"
                  onClick={() => toggleGroup(group.title)}
                >
                  {ICONS[group.icon]}
                  {group.title}
                  <span
                    style={{
                      marginLeft: "auto",
                      fontSize: "0.6rem",
                      color: "var(--text-tertiary)",
                      fontWeight: 400,
                      fontFamily: "var(--font-mono)",
                    }}
                  >
                    {getGroupSummary(group.fields)}
                  </span>
                </h3>
                <div className="config-group-body">
                  {group.fields.map((field) => (
                    <div key={field.key} className="config-row">
                      <label className="config-label" htmlFor={field.key}>
                        {field.label}
                        {field.restart ? (
                          <span className="badge-restart">restart</span>
                        ) : (
                          <span
                            style={{
                              fontSize: "0.58rem",
                              fontFamily: "var(--font-mono)",
                              padding: "1px 6px",
                              borderRadius: 20,
                              background: "var(--green-dim)",
                              color: "var(--green)",
                              whiteSpace: "nowrap",
                            }}
                          >
                            live
                          </span>
                        )}
                      </label>
                      {field.type === "select" ? (
                        <select
                          id={field.key}
                          className="config-input"
                          value={String(values[field.key] ?? "")}
                          onChange={(e) =>
                            handleChange(field.key, e.target.value)
                          }
                        >
                          {field.options?.map((opt) => (
                            <option key={opt} value={opt}>
                              {opt}
                            </option>
                          ))}
                        </select>
                      ) : field.type === "checkbox" ? (
                        <input
                          id={field.key}
                          type="checkbox"
                          className="config-input"
                          checked={!!values[field.key]}
                          onChange={(e) =>
                            handleChange(field.key, e.target.checked)
                          }
                        />
                      ) : field.type === "password" ? (
                        <>
                          <input
                            id={field.key}
                            type={revealPassword ? "text" : "password"}
                            className="config-input"
                            value={String(values[field.key] ?? "")}
                            onChange={(e) =>
                              handleChange(field.key, e.target.value)
                            }
                          />
                          <button
                            type="button"
                            className="btn-reveal"
                            aria-label={
                              revealPassword
                                ? "Hide password"
                                : "Reveal password"
                            }
                            aria-pressed={revealPassword}
                            onClick={() => setRevealPassword((v) => !v)}
                          >
                            {revealPassword ? "hide" : "reveal"}
                          </button>
                        </>
                      ) : (
                        <input
                          id={field.key}
                          type={field.type}
                          className="config-input"
                          value={String(values[field.key] ?? "")}
                          onChange={(e) =>
                            handleChange(
                              field.key,
                              field.type === "number"
                                ? Number(e.target.value)
                                : e.target.value,
                            )
                          }
                        />
                      )}
                    </div>
                  ))}
                </div>
              </div>
            ))}
            {dirty && (
              <div className="config-save-bar">
                <button type="submit" className="btn-save">
                  Save Configuration
                </button>
                <button
                  type="button"
                  className="btn-reveal"
                  onClick={handleReset}
                >
                  Reset
                </button>
                <span
                  style={{
                    fontSize: "0.7rem",
                    color: "var(--yellow)",
                    fontFamily: "var(--font-mono)",
                  }}
                >
                  {changedKeysSet.size} unsaved{" "}
                  {changedKeysSet.size === 1 ? "change" : "changes"}
                </span>
              </div>
            )}
            {!dirty && (
              <div className="btn-save-wrap">
                <button type="submit" className="btn-save" disabled>
                  Save Configuration
                </button>
              </div>
            )}
          </div>

          <div className="history-panel">
            <div className="history-panel-header">Change History</div>
            <div className="history-list">
              {history.length === 0 ? (
                <div className="empty-state">No changes recorded</div>
              ) : (
                history.map((h, i) => (
                  <div key={i} className="history-item">
                    <div className="history-item-time">
                      {h.changed_at || h.timestamp || ""}
                    </div>
                    <div className="history-item-field">
                      {h.key || h.field || ""}
                    </div>
                    <div className="history-item-diff">
                      <span className="old-val">
                        {String(h.old_value ?? "")}
                      </span>
                      {" \u2192 "}
                      <span className="new-val">
                        {String(h.new_value ?? "")}
                      </span>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </form>
    </>
  );
}
