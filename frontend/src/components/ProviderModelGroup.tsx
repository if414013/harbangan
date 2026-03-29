import { useState } from "react";
import { DataTable } from "./DataTable";
import { ModelAllowlistEditor } from "./ModelAllowlistEditor";
import type { RegistryModel } from "../lib/api";
import type { ProviderGroup } from "../lib/providers";

interface ProviderModelGroupProps {
  group: ProviderGroup;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
  onPopulate: (providerId: string) => void;
  isAdmin?: boolean;
  allowedModelIds?: string[];
  onSaveDefaults?: (providerId: string, modelIds: string[]) => Promise<void>;
  onApplyDefaults?: (providerId: string) => Promise<void>;
  onClearDefaults?: (providerId: string) => Promise<void>;
}

export function ProviderModelGroup({
  group,
  onToggle,
  onDelete,
  onPopulate,
  isAdmin,
  allowedModelIds = [],
  onSaveDefaults,
  onApplyDefaults,
  onClearDefaults,
}: ProviderModelGroupProps) {
  const [collapsed, setCollapsed] = useState(false);
  const visibleModels = isAdmin
    ? group.models
    : group.models.filter((m) => m.enabled);
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
          {isAdmin
            ? `${enabledCount}/${group.models.length} enabled`
            : `${visibleModels.length} models`}
        </span>
      </div>
      <div className="config-group-body">
        {isAdmin && (
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
        )}
        <DataTable
          data={visibleModels as unknown as Record<string, unknown>[]}
          columns={[
            ...(isAdmin
              ? [
                  {
                    key: "enabled",
                    label: "enabled",
                    sortable: true,
                    render: (row: Record<string, unknown>) => {
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
                ]
              : []),
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
            ...(isAdmin
              ? [
                  {
                    key: "id",
                    label: "",
                    render: (row: Record<string, unknown>) => {
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
                ]
              : []),
          ]}
          searchKeys={["display_name", "prefixed_id"]}
          searchPlaceholder="Search models..."
          emptyTitle="No models"
          caption={`Models for ${group.providerId}`}
        />
        {isAdmin && onSaveDefaults && onApplyDefaults && onClearDefaults && (
          <ModelAllowlistEditor
            models={group.models}
            allowedModelIds={allowedModelIds}
            onSave={(modelIds) => onSaveDefaults(group.providerId, modelIds)}
            onApply={() => onApplyDefaults(group.providerId)}
            onClear={() => onClearDefaults(group.providerId)}
          />
        )}
      </div>
    </div>
  );
}
