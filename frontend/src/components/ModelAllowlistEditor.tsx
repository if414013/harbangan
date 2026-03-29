import { useState } from "react";
import type { RegistryModel } from "../lib/api";

interface ModelAllowlistEditorProps {
  models: RegistryModel[];
  allowedModelIds: string[];
  onSave: (modelIds: string[]) => Promise<void>;
  onApply: () => Promise<void>;
  onClear: () => Promise<void>;
}

export function ModelAllowlistEditor({
  models,
  allowedModelIds,
  onSave,
  onApply,
  onClear,
}: ModelAllowlistEditorProps) {
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(allowedModelIds),
  );
  const [saving, setSaving] = useState(false);
  const [applying, setApplying] = useState(false);

  const hasDefaults = allowedModelIds.length > 0;
  const isDirty =
    selected.size !== allowedModelIds.length ||
    allowedModelIds.some((id) => !selected.has(id));

  function handleToggle(modelId: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(modelId)) {
        next.delete(modelId);
      } else {
        next.add(modelId);
      }
      return next;
    });
  }

  function handleSelectAll() {
    setSelected(new Set(models.map((m) => m.model_id)));
  }

  function handleSelectNone() {
    setSelected(new Set());
  }

  async function handleSave() {
    setSaving(true);
    try {
      await onSave(Array.from(selected));
    } finally {
      setSaving(false);
    }
  }

  async function handleApply() {
    setApplying(true);
    try {
      await onApply();
    } finally {
      setApplying(false);
    }
  }

  return (
    <div
      className="allowlist-editor"
      style={{
        padding: "12px 16px",
        borderTop: "1px solid var(--border-subtle)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 8,
        }}
      >
        <span
          style={{
            fontSize: "0.65rem",
            color: "var(--cyan)",
            textTransform: "uppercase",
            letterSpacing: "0.05em",
          }}
        >
          visibility defaults
        </span>
        <div style={{ display: "flex", gap: 6 }}>
          <button
            className="btn-reveal"
            type="button"
            onClick={handleSelectAll}
          >
            all
          </button>
          <button
            className="btn-reveal"
            type="button"
            onClick={handleSelectNone}
          >
            none
          </button>
        </div>
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
          gap: "4px 12px",
          maxHeight: 200,
          overflowY: "auto",
          marginBottom: 10,
        }}
      >
        {models.map((m) => (
          <label
            key={m.model_id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontSize: "0.7rem",
              color: selected.has(m.model_id)
                ? "var(--text)"
                : "var(--text-tertiary)",
              cursor: "pointer",
              padding: "2px 0",
            }}
          >
            <input
              type="checkbox"
              checked={selected.has(m.model_id)}
              onChange={() => handleToggle(m.model_id)}
              style={{ accentColor: "var(--green)" }}
            />
            {m.display_name || m.model_id}
          </label>
        ))}
      </div>
      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
        <button
          className="btn-save"
          type="button"
          onClick={handleSave}
          disabled={saving || !isDirty}
        >
          {saving ? "saving..." : "save defaults"}
        </button>
        <button
          className="btn-reveal"
          type="button"
          onClick={handleApply}
          disabled={applying || !hasDefaults}
          title="Enable only allowlisted models, disable the rest"
        >
          {applying ? "applying..." : "apply now"}
        </button>
        {hasDefaults && (
          <button className="btn-reveal" type="button" onClick={onClear}>
            clear
          </button>
        )}
        {isDirty && (
          <span
            style={{
              fontSize: "0.6rem",
              color: "var(--yellow)",
              marginLeft: 4,
            }}
          >
            unsaved
          </span>
        )}
        <span
          style={{
            marginLeft: "auto",
            fontSize: "0.6rem",
            color: "var(--text-tertiary)",
          }}
        >
          {selected.size}/{models.length} selected
        </span>
      </div>
    </div>
  );
}
