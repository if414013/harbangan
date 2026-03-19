import { ConfirmDialog } from "../../components/ConfirmDialog";
import { ProviderModelGroup } from "../../components/ProviderModelGroup";
import { groupByProvider } from "../../lib/providers";
import type { RegistryModel } from "../../lib/api";

interface ModelsTabProps {
  models: RegistryModel[];
  modelsLoading: boolean;
  populating: boolean;
  confirmState: {
    action: () => void;
    title: string;
    message: string;
  } | null;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
  onPopulate: (providerId?: string) => void;
  onConfirm: () => void;
  onCancelConfirm: () => void;
}

export function ModelsTab({
  models,
  modelsLoading,
  populating,
  confirmState,
  onToggle,
  onDelete,
  onPopulate,
  onConfirm,
  onCancelConfirm,
}: ModelsTabProps) {
  const groups = groupByProvider(models);

  if (modelsLoading) {
    return (
      <div
        className="skeleton skeleton-block"
        role="status"
        aria-label="Loading models"
      />
    );
  }

  return (
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
          onClick={() => onPopulate()}
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
            No models in registry. Click &quot;populate all&quot; to fetch
            models from connected providers.
          </div>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {groups.map((g) => (
            <ProviderModelGroup
              key={g.providerId}
              group={g}
              onToggle={onToggle}
              onDelete={onDelete}
              onPopulate={onPopulate}
            />
          ))}
        </div>
      )}
      {confirmState && (
        <ConfirmDialog
          title={confirmState.title}
          message={confirmState.message}
          confirmLabel="Delete"
          variant="danger"
          onConfirm={onConfirm}
          onCancel={onCancelConfirm}
        />
      )}
    </>
  );
}
