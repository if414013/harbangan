import { useState } from "react";
import { DataTable } from "./DataTable";
import type { RegistryModel } from "../lib/api";
import type { ProviderGroup } from "../lib/providers";

interface ProviderModelGroupProps {
  group: ProviderGroup;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
  onPopulate: (providerId: string) => void;
}

export function ProviderModelGroup({
  group,
  onToggle,
  onDelete,
  onPopulate,
}: ProviderModelGroupProps) {
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
        <DataTable
          data={group.models as unknown as Record<string, unknown>[]}
          columns={[
            {
              key: "enabled",
              label: "enabled",
              sortable: true,
              render: (row) => {
                const m = row as unknown as RegistryModel;
                return (
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
                );
              },
            },
            {
              key: "prefixed_id",
              label: "prefixed id",
              sortable: true,
            },
            {
              key: "display_name",
              label: "display name",
              sortable: true,
              render: (row) => (
                <span style={{ color: "var(--text-secondary)" }}>
                  {String(row.display_name ?? "")}
                </span>
              ),
            },
            {
              key: "context_length",
              label: "context",
              sortable: true,
              render: (row) => (
                <span style={{ color: "var(--text-tertiary)" }}>
                  {(row.context_length as number).toLocaleString()}
                </span>
              ),
            },
            {
              key: "source",
              label: "source",
              render: (row) => (
                <span className="source-badge">{String(row.source ?? "")}</span>
              ),
            },
            {
              key: "id",
              label: "",
              render: (row) => {
                const m = row as unknown as RegistryModel;
                return (
                  <button
                    className="btn-danger"
                    type="button"
                    onClick={() => onDelete(m.id)}
                    aria-label={`Delete ${m.prefixed_id}`}
                  >
                    delete
                  </button>
                );
              },
            },
          ]}
          searchKeys={["display_name", "prefixed_id"]}
          searchPlaceholder="Search models..."
          emptyTitle="No models"
          caption={`Models for ${group.providerId}`}
        />
      </div>
    </div>
  );
}
