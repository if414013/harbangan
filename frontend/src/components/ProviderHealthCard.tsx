import type { RateLimitInfo } from "../lib/api";

interface ProviderHealthCardProps {
  name: string;
  providerId: string;
  connected: boolean;
  enabled: boolean;
  isAdmin: boolean;
  modelCount: number;
  accountCount: number;
  rateLimits: RateLimitInfo[];
  onClick: () => void;
  onToggle: (enabled: boolean) => void;
}

export function ProviderHealthCard({
  name,
  providerId,
  connected,
  enabled,
  isAdmin,
  modelCount,
  accountCount,
  rateLimits,
  onClick,
  onToggle,
}: ProviderHealthCardProps) {
  const limitedCount = rateLimits.filter((r) => r.limited_until != null).length;

  return (
    <button
      type="button"
      className="health-card"
      onClick={onClick}
      data-connected={connected ? "true" : "false"}
    >
      <div className="health-card-header">
        <span className="health-card-name">
          {"> "}
          {name}
        </span>
        {isAdmin && providerId !== "kiro" && (
          <button
            type="button"
            className="role-badge"
            onClick={(e) => {
              e.stopPropagation();
              onToggle(!enabled);
            }}
            aria-label={`Toggle ${name} ${enabled ? "off" : "on"}`}
            style={{
              background: enabled ? "var(--green-dim)" : "var(--red-dim)",
              color: enabled ? "var(--green)" : "var(--red)",
            }}
          >
            {enabled ? "on" : "off"}
          </button>
        )}
        <span className="health-card-dot" />
      </div>
      <div className="health-card-status">
        {connected ? "Connected" : "Offline"}
      </div>
      {connected && (
        <div className="health-card-meta">
          {modelCount > 0 && (
            <span>
              {modelCount} model{modelCount !== 1 ? "s" : ""}
            </span>
          )}
          {accountCount > 1 && <span>{accountCount} accounts</span>}
          {limitedCount > 0 && (
            <span className="health-card-warn">{limitedCount} limited</span>
          )}
        </div>
      )}
    </button>
  );
}
