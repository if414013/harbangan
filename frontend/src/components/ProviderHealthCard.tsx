import type { RateLimitInfo } from "../lib/api";

interface ProviderHealthCardProps {
  name: string;
  providerId: string;
  connected: boolean;
  modelCount: number;
  accountCount: number;
  rateLimits: RateLimitInfo[];
  onClick: () => void;
}

export function ProviderHealthCard({
  name,
  connected,
  modelCount,
  accountCount,
  rateLimits,
  onClick,
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
