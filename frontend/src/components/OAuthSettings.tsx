import { useState, useEffect } from "react";
import { apiFetch, apiPut } from "../lib/api";
import { useToast } from "./useToast";

const OAUTH_FIELDS = [
  { key: "anthropic_oauth_client_id", label: "Anthropic OAuth Client ID" },
  { key: "openai_oauth_client_id", label: "OpenAI OAuth Client ID" },
] as const;

export function OAuthSettings() {
  const { showToast } = useToast();
  const [values, setValues] = useState<Record<string, string>>({});
  const [saved, setSaved] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    apiFetch<Record<string, unknown>>("/config")
      .then((config) => {
        const initial: Record<string, string> = {};
        for (const f of OAUTH_FIELDS) {
          initial[f.key] = String(config[f.key] ?? "");
        }
        setValues(initial);
        setSaved(initial);
      })
      .catch(() => {});
  }, []);

  const hasChanges = OAUTH_FIELDS.some(
    (f) => (values[f.key] ?? "") !== (saved[f.key] ?? ""),
  );

  async function handleSave() {
    setSaving(true);
    try {
      const payload: Record<string, unknown> = {};
      for (const f of OAUTH_FIELDS) {
        payload[f.key] = values[f.key] ?? "";
      }
      await apiPut("/config", payload);
      setSaved({ ...values });
      showToast("OAuth settings saved", "success");
    } catch (err) {
      showToast(err instanceof Error ? err.message : "Failed to save", "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="config-group">
      <div className="config-group-header">
        <span>OAuth Settings</span>
      </div>
      <div className="config-group-body">
        {OAUTH_FIELDS.map((f) => (
          <div key={f.key} className="config-row">
            <label className="config-label" htmlFor={f.key}>
              {f.label}
            </label>
            <input
              id={f.key}
              className="config-input"
              type="text"
              value={values[f.key] ?? ""}
              onChange={(e) =>
                setValues((prev) => ({ ...prev, [f.key]: e.target.value }))
              }
            />
          </div>
        ))}
        <div style={{ padding: "8px 16px" }}>
          <button
            className="btn-save"
            type="button"
            onClick={handleSave}
            disabled={saving || !hasChanges}
          >
            {saving ? "saving..." : "$ save"}
          </button>
        </div>
      </div>
    </div>
  );
}
