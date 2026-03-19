import { useState, useEffect } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PageHeader } from "../components/PageHeader";
import {
  getGuardrailProfiles,
  createGuardrailProfile,
  updateGuardrailProfile,
  deleteGuardrailProfile,
  getGuardrailRules,
  createGuardrailRule,
  updateGuardrailRule,
  deleteGuardrailRule,
  testGuardrailProfile,
  validateCelExpression,
} from "../lib/api";
import type { GuardrailProfile, GuardrailRule } from "../lib/api";
import { useToast } from "../components/useToast";

// --- Profiles Sub-component ---

function GuardrailProfiles() {
  const { showToast } = useToast();
  const [profiles, setProfiles] = useState<GuardrailProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);
  const [form, setForm] = useState({
    name: "",
    guardrail_id: "",
    guardrail_version: "1",
    region: "us-east-1",
    access_key: "",
    secret_key: "",
  });

  function loadProfiles() {
    getGuardrailProfiles()
      .then((data) => {
        setProfiles(data);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }

  useEffect(() => {
    loadProfiles();
  }, []);

  function resetForm() {
    setForm({
      name: "",
      guardrail_id: "",
      guardrail_version: "1",
      region: "us-east-1",
      access_key: "",
      secret_key: "",
    });
    setEditId(null);
    setShowForm(false);
  }

  function startEdit(p: GuardrailProfile) {
    setForm({
      name: p.name,
      guardrail_id: p.guardrail_id,
      guardrail_version: p.guardrail_version,
      region: p.region,
      access_key: p.access_key,
      secret_key: "",
    });
    setEditId(p.id);
    setShowForm(true);
  }

  async function handleSubmit() {
    if (!form.name.trim() || !form.guardrail_id.trim()) {
      showToast("Name and Guardrail ID are required", "error");
      return;
    }
    try {
      if (editId) {
        const payload: Record<string, unknown> = { ...form };
        if (!form.secret_key) delete payload.secret_key;
        await updateGuardrailProfile(editId, payload);
        showToast("Profile updated", "success");
      } else {
        await createGuardrailProfile(form);
        showToast("Profile created", "success");
      }
      resetForm();
      loadProfiles();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to save profile",
        "error",
      );
    }
  }

  function handleDelete(id: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteGuardrailProfile(id);
          showToast("Profile deleted", "success");
          loadProfiles();
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to delete profile",
            "error",
          );
        }
      },
      title: "Delete profile",
      message: "Delete this profile? This cannot be undone.",
    });
  }

  async function handleToggle(p: GuardrailProfile) {
    try {
      await updateGuardrailProfile(p.id, { enabled: !p.enabled });
      showToast(`Profile ${p.enabled ? "disabled" : "enabled"}`, "success");
      loadProfiles();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to toggle profile",
        "error",
      );
    }
  }

  if (loading) {
    return (
      <div
        className="skeleton skeleton-block"
        role="status"
        aria-label="Loading profiles"
      />
    );
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{"> "}profiles</span>
        <span className="card-subtitle">{profiles.length} total</span>
      </div>
      {profiles.length === 0 && !showForm ? (
        <div className="empty-state">No guardrail profiles configured</div>
      ) : (
        <table className="data-table">
          <caption className="sr-only">Guardrail Profiles</caption>
          <thead>
            <tr>
              <th scope="col">name</th>
              <th scope="col">guardrail id</th>
              <th scope="col">region</th>
              <th scope="col">enabled</th>
              <th scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {profiles.map((p) => (
              <tr key={p.id}>
                <td>{p.name}</td>
                <td>
                  {p.guardrail_id} v{p.guardrail_version}
                </td>
                <td>{p.region}</td>
                <td>
                  <button
                    type="button"
                    className="role-badge"
                    onClick={() => handleToggle(p)}
                    aria-label={`Toggle profile ${p.name} ${p.enabled ? "off" : "on"}`}
                    style={{
                      background: p.enabled
                        ? "var(--green-dim)"
                        : "var(--red-dim)",
                      color: p.enabled ? "var(--green)" : "var(--red)",
                    }}
                  >
                    {p.enabled ? "on" : "off"}
                  </button>
                </td>
                <td style={{ display: "flex", gap: 8 }}>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => startEdit(p)}
                    aria-label={`Edit profile ${p.name}`}
                  >
                    edit
                  </button>
                  <button
                    className="btn-danger"
                    type="button"
                    onClick={() => handleDelete(p.id)}
                    aria-label={`Delete profile ${p.name}`}
                  >
                    delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {showForm && (
        <div className="guardrails-form">
          <div className="guardrails-form-grid">
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="profile-name">
                Name
              </label>
              <input
                id="profile-name"
                className="config-input"
                autoFocus
                value={form.name}
                onChange={(e) =>
                  setForm((f) => ({ ...f, name: e.target.value }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="profile-gid">
                Guardrail ID
              </label>
              <input
                id="profile-gid"
                className="config-input"
                value={form.guardrail_id}
                onChange={(e) =>
                  setForm((f) => ({ ...f, guardrail_id: e.target.value }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="profile-ver">
                Version
              </label>
              <input
                id="profile-ver"
                className="config-input"
                value={form.guardrail_version}
                onChange={(e) =>
                  setForm((f) => ({ ...f, guardrail_version: e.target.value }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="profile-region">
                Region
              </label>
              <input
                id="profile-region"
                className="config-input"
                value={form.region}
                onChange={(e) =>
                  setForm((f) => ({ ...f, region: e.target.value }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="profile-ak">
                Access Key
              </label>
              <input
                id="profile-ak"
                className="config-input"
                value={form.access_key}
                onChange={(e) =>
                  setForm((f) => ({ ...f, access_key: e.target.value }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="profile-sk">
                Secret Key
              </label>
              <input
                id="profile-sk"
                className="config-input"
                type="password"
                value={form.secret_key}
                onChange={(e) =>
                  setForm((f) => ({ ...f, secret_key: e.target.value }))
                }
                placeholder={editId ? "(unchanged)" : ""}
              />
            </div>
          </div>
          <div className="guardrails-form-actions">
            <button className="btn-save" type="button" onClick={handleSubmit}>
              {editId ? "$ update" : "$ create"}
            </button>
            <button
              className="device-code-cancel"
              type="button"
              onClick={resetForm}
            >
              cancel
            </button>
          </div>
        </div>
      )}
      {!showForm && (
        <div style={{ marginTop: 12 }}>
          <button
            className="btn-save"
            type="button"
            onClick={() => setShowForm(true)}
          >
            $ new profile
          </button>
        </div>
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
    </div>
  );
}

// --- Rules Sub-component ---

function GuardrailRules() {
  const { showToast } = useToast();
  const [rules, setRules] = useState<GuardrailRule[]>([]);
  const [profiles, setProfiles] = useState<GuardrailProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [celStatus, setCelStatus] = useState<string | null>(null);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);
  const [form, setForm] = useState({
    name: "",
    description: "",
    cel_expression: "",
    apply_to: "both" as "input" | "output" | "both",
    sampling_rate: 100,
    timeout_ms: 5000,
    profile_ids: [] as string[],
  });

  function loadRules() {
    Promise.all([getGuardrailRules(), getGuardrailProfiles()])
      .then(([r, p]) => {
        setRules(r);
        setProfiles(p);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }

  useEffect(() => {
    loadRules();
  }, []);

  function resetForm() {
    setForm({
      name: "",
      description: "",
      cel_expression: "",
      apply_to: "both",
      sampling_rate: 100,
      timeout_ms: 5000,
      profile_ids: [],
    });
    setEditId(null);
    setShowForm(false);
    setCelStatus(null);
  }

  function startEdit(r: GuardrailRule) {
    setForm({
      name: r.name,
      description: r.description,
      cel_expression: r.cel_expression,
      apply_to: r.apply_to,
      sampling_rate: r.sampling_rate,
      timeout_ms: r.timeout_ms,
      profile_ids: r.profile_ids,
    });
    setEditId(r.id);
    setShowForm(true);
    setCelStatus(null);
  }

  async function handleValidateCel() {
    if (!form.cel_expression.trim()) {
      setCelStatus("empty expression matches all requests");
      return;
    }
    try {
      const result = await validateCelExpression(form.cel_expression);
      setCelStatus(result.valid ? "valid" : `error: ${result.error}`);
    } catch (err) {
      setCelStatus(err instanceof Error ? err.message : "validation failed");
    }
  }

  async function handleSubmit() {
    if (!form.name.trim()) {
      showToast("Name is required", "error");
      return;
    }
    if (form.profile_ids.length === 0) {
      showToast("Select at least one profile", "error");
      return;
    }
    try {
      if (editId) {
        await updateGuardrailRule(editId, form);
        showToast("Rule updated", "success");
      } else {
        await createGuardrailRule(form);
        showToast("Rule created", "success");
      }
      resetForm();
      loadRules();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to save rule",
        "error",
      );
    }
  }

  function handleDelete(id: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteGuardrailRule(id);
          showToast("Rule deleted", "success");
          loadRules();
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to delete rule",
            "error",
          );
        }
      },
      title: "Delete rule",
      message: "Delete this rule? This cannot be undone.",
    });
  }

  async function handleToggle(r: GuardrailRule) {
    try {
      await updateGuardrailRule(r.id, { enabled: !r.enabled });
      showToast(`Rule ${r.enabled ? "disabled" : "enabled"}`, "success");
      loadRules();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to toggle rule",
        "error",
      );
    }
  }

  function toggleProfile(id: string) {
    setForm((f) => ({
      ...f,
      profile_ids: f.profile_ids.includes(id)
        ? f.profile_ids.filter((p) => p !== id)
        : [...f.profile_ids, id],
    }));
  }

  if (loading) {
    return (
      <div
        className="skeleton skeleton-block"
        role="status"
        aria-label="Loading rules"
      />
    );
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{"> "}rules</span>
        <span className="card-subtitle">{rules.length} total</span>
      </div>
      {rules.length === 0 && !showForm ? (
        <div className="empty-state">No guardrail rules configured</div>
      ) : (
        <table className="data-table">
          <caption className="sr-only">Guardrail Rules</caption>
          <thead>
            <tr>
              <th scope="col">name</th>
              <th scope="col">apply to</th>
              <th scope="col">sampling</th>
              <th scope="col">profiles</th>
              <th scope="col">enabled</th>
              <th scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rules.map((r) => (
              <tr key={r.id}>
                <td>{r.name}</td>
                <td>{r.apply_to}</td>
                <td>{r.sampling_rate}%</td>
                <td>{r.profile_ids.length}</td>
                <td>
                  <button
                    type="button"
                    className="role-badge"
                    onClick={() => handleToggle(r)}
                    aria-label={`Toggle rule ${r.name} ${r.enabled ? "off" : "on"}`}
                    style={{
                      background: r.enabled
                        ? "var(--green-dim)"
                        : "var(--red-dim)",
                      color: r.enabled ? "var(--green)" : "var(--red)",
                    }}
                  >
                    {r.enabled ? "on" : "off"}
                  </button>
                </td>
                <td style={{ display: "flex", gap: 8 }}>
                  <button
                    className="device-code-cancel"
                    type="button"
                    onClick={() => startEdit(r)}
                    aria-label={`Edit rule ${r.name}`}
                  >
                    edit
                  </button>
                  <button
                    className="btn-danger"
                    type="button"
                    onClick={() => handleDelete(r.id)}
                    aria-label={`Delete rule ${r.name}`}
                  >
                    delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {showForm && (
        <div className="guardrails-form">
          <div className="guardrails-form-grid">
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="rule-name">
                Name
              </label>
              <input
                id="rule-name"
                className="config-input"
                autoFocus
                value={form.name}
                onChange={(e) =>
                  setForm((f) => ({ ...f, name: e.target.value }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="rule-desc">
                Description
              </label>
              <input
                id="rule-desc"
                className="config-input"
                value={form.description}
                onChange={(e) =>
                  setForm((f) => ({ ...f, description: e.target.value }))
                }
              />
            </div>
            <div
              className="guardrails-form-field"
              style={{ gridColumn: "1 / -1" }}
            >
              <label className="config-label" htmlFor="rule-cel">
                CEL Expression
              </label>
              <div style={{ display: "flex", gap: 8, alignItems: "start" }}>
                <textarea
                  id="rule-cel"
                  className="config-input guardrails-cel-input"
                  value={form.cel_expression}
                  onChange={(e) => {
                    setForm((f) => ({ ...f, cel_expression: e.target.value }));
                    setCelStatus(null);
                  }}
                  placeholder='e.g. request.model == "claude-sonnet-4-20250514"'
                  rows={2}
                />
                <button
                  className="btn-save"
                  type="button"
                  onClick={handleValidateCel}
                  style={{ flexShrink: 0 }}
                >
                  validate
                </button>
              </div>
              <div aria-live="polite">
                {celStatus !== null && (
                  <div
                    style={{
                      fontSize: "0.68rem",
                      fontFamily: "var(--font-mono)",
                      marginTop: 4,
                      color: celStatus.startsWith("error")
                        ? "var(--red)"
                        : "var(--green)",
                    }}
                  >
                    {celStatus}
                  </div>
                )}
              </div>
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="rule-apply">
                Apply To
              </label>
              <select
                id="rule-apply"
                className="config-input"
                value={form.apply_to}
                onChange={(e) =>
                  setForm((f) => ({
                    ...f,
                    apply_to: e.target.value as "input" | "output" | "both",
                  }))
                }
              >
                <option value="both">both</option>
                <option value="input">input</option>
                <option value="output">output</option>
              </select>
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="rule-sampling">
                Sampling Rate (%)
              </label>
              <input
                id="rule-sampling"
                className="config-input"
                type="number"
                min={0}
                max={100}
                value={form.sampling_rate}
                onChange={(e) =>
                  setForm((f) => ({
                    ...f,
                    sampling_rate: Number(e.target.value),
                  }))
                }
              />
            </div>
            <div className="guardrails-form-field">
              <label className="config-label" htmlFor="rule-timeout">
                Timeout (ms)
              </label>
              <input
                id="rule-timeout"
                className="config-input"
                type="number"
                min={100}
                max={30000}
                value={form.timeout_ms}
                onChange={(e) =>
                  setForm((f) => ({ ...f, timeout_ms: Number(e.target.value) }))
                }
              />
            </div>
            <div
              className="guardrails-form-field"
              style={{ gridColumn: "1 / -1" }}
            >
              <label className="config-label">Linked Profiles</label>
              {profiles.length === 0 ? (
                <div className="empty-state" style={{ padding: "8px 0" }}>
                  No profiles available — create one first
                </div>
              ) : (
                <div className="guardrails-profile-select">
                  {profiles.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      className={`pill${form.profile_ids.includes(p.id) ? " active" : ""}`}
                      aria-pressed={form.profile_ids.includes(p.id)}
                      onClick={() => toggleProfile(p.id)}
                    >
                      {p.name}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
          <div className="guardrails-form-actions">
            <button className="btn-save" type="button" onClick={handleSubmit}>
              {editId ? "$ update" : "$ create"}
            </button>
            <button
              className="device-code-cancel"
              type="button"
              onClick={resetForm}
            >
              cancel
            </button>
          </div>
        </div>
      )}
      {!showForm && (
        <div style={{ marginTop: 12 }}>
          <button
            className="btn-save"
            type="button"
            onClick={() => {
              setShowForm(true);
              setCelStatus(null);
            }}
          >
            $ new rule
          </button>
        </div>
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
    </div>
  );
}

// --- Test Sub-component ---

function GuardrailTester() {
  const { showToast } = useToast();
  const [profiles, setProfiles] = useState<GuardrailProfile[]>([]);
  const [selectedProfile, setSelectedProfile] = useState("");
  const [content, setContent] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{
    action: string;
    response_time_ms: number;
    error?: string;
  } | null>(null);

  useEffect(() => {
    getGuardrailProfiles()
      .then((data) => {
        setProfiles(data);
        if (data.length > 0) setSelectedProfile(data[0].id);
      })
      .catch(() => {});
  }, []);

  async function handleTest() {
    if (!selectedProfile || !content.trim()) {
      showToast("Select a profile and enter test content", "error");
      return;
    }
    setTesting(true);
    setResult(null);
    try {
      const res = await testGuardrailProfile(selectedProfile, content);
      setResult(res);
    } catch (err) {
      showToast(err instanceof Error ? err.message : "Test failed", "error");
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="card">
      <div className="card-header">
        <span className="card-title">{"> "}test</span>
      </div>
      <div className="guardrails-form">
        <div className="guardrails-form-grid">
          <div className="guardrails-form-field">
            <label className="config-label" htmlFor="test-profile">
              Profile
            </label>
            <select
              id="test-profile"
              className="config-input"
              value={selectedProfile}
              onChange={(e) => setSelectedProfile(e.target.value)}
            >
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
          <div
            className="guardrails-form-field"
            style={{ gridColumn: "1 / -1" }}
          >
            <label className="config-label" htmlFor="test-content">
              Content
            </label>
            <textarea
              id="test-content"
              className="config-input guardrails-cel-input"
              value={content}
              onChange={(e) => setContent(e.target.value)}
              placeholder="Enter text to test against the guardrail..."
              rows={3}
            />
          </div>
        </div>
        <div className="guardrails-form-actions">
          <button
            className="btn-save"
            type="button"
            onClick={handleTest}
            disabled={testing || !selectedProfile}
          >
            {testing ? "testing..." : "$ test"}
          </button>
        </div>
        <div aria-live="polite">
          {result && (
            <div
              className="guardrails-test-result"
              style={{
                color: result.action === "NONE" ? "var(--green)" : "var(--red)",
              }}
            >
              <span>action: {result.action}</span>
              <span>time: {result.response_time_ms}ms</span>
              {result.error && <span>error: {result.error}</span>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// --- Main Page ---

export function Guardrails() {
  return (
    <>
      <PageHeader
        title="guardrails"
        description="Configure content safety profiles and routing rules."
      />
      <h2 className="section-header">PROFILES</h2>
      <div className="mb-24">
        <GuardrailProfiles />
      </div>

      <h2 className="section-header">RULES</h2>
      <div className="mb-24">
        <GuardrailRules />
      </div>

      <h2 className="section-header">TEST GUARDRAIL</h2>
      <GuardrailTester />
    </>
  );
}
