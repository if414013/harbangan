import { ProviderHealthCard } from "../../components/ProviderHealthCard";
import { providerDisplayName } from "../../lib/providers";
import type {
  ProvidersStatusResponse,
  ProviderRegistryEntry,
  RegistryModel,
  UserProviderAccount,
  RateLimitInfo,
} from "../../lib/api";

interface StatusTabProps {
  providerStatus: ProvidersStatusResponse | null;
  models: RegistryModel[];
  providerAccounts: Record<string, UserProviderAccount[]>;
  rateLimits: RateLimitInfo[];
  kiroConnected: boolean;
  copilotConnected: boolean;
  onNavigate: (providerId: string) => void;
  allProviders: string[];
  registry: ProviderRegistryEntry[];
}

export function StatusTab({
  providerStatus,
  models,
  providerAccounts,
  rateLimits,
  kiroConnected,
  copilotConnected,
  onNavigate,
  allProviders,
  registry,
}: StatusTabProps) {
  function isConnected(id: string): boolean {
    if (id === "kiro") return kiroConnected;
    if (id === "copilot") return copilotConnected;
    return providerStatus?.providers[id]?.connected ?? false;
  }

  function getModelCount(id: string): number {
    return models.filter((m) => m.provider_id === id).length;
  }

  function getAccountCount(id: string): number {
    return (providerAccounts[id] ?? []).length;
  }

  function getRateLimits(id: string): RateLimitInfo[] {
    return rateLimits.filter((r) => r.provider_id === id);
  }

  const connectedCount = allProviders.filter((p) => isConnected(p)).length;
  const enabledModels = models.filter((m) => m.enabled).length;

  return (
    <>
      <div className="health-grid">
        {allProviders.map((p) => (
          <ProviderHealthCard
            key={p}
            name={providerDisplayName(p, registry)}
            providerId={p}
            connected={isConnected(p)}
            modelCount={getModelCount(p)}
            accountCount={getAccountCount(p)}
            rateLimits={getRateLimits(p)}
            onClick={() => onNavigate(p)}
          />
        ))}
      </div>
      <div className="summary-bar">
        <span className="summary-bar-stat">
          <strong>{enabledModels}</strong> models enabled / {models.length}{" "}
          total
        </span>
        <span className="summary-bar-stat">
          <strong>{connectedCount}</strong>/{allProviders.length} providers
          connected
        </span>
      </div>
    </>
  );
}
