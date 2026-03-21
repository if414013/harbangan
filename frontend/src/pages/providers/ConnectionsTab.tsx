import type { ComponentType } from "react";
import { KiroSetup } from "../../components/KiroSetup";
import { CopilotSetup } from "../../components/CopilotSetup";
import { ProviderCard } from "../../components/ProviderCard";
import { OAuthSettings } from "../../components/OAuthSettings";
import type {
  ProvidersStatusResponse,
  UserProviderAccount,
  RateLimitInfo,
} from "../../lib/api";

const DEVICE_CODE_COMPONENTS: Record<string, ComponentType> = {
  kiro: KiroSetup,
  copilot: CopilotSetup,
};

interface ConnectionsTabProps {
  providerStatus: ProvidersStatusResponse | null;
  providerAccounts: Record<string, UserProviderAccount[]>;
  rateLimits: RateLimitInfo[];
  isAdmin: boolean;
  onRefresh: () => void;
  multiAccountProviders: string[];
  deviceCodeProviders: string[];
}

export function ConnectionsTab({
  providerStatus,
  providerAccounts,
  rateLimits,
  isAdmin,
  onRefresh,
  multiAccountProviders,
  deviceCodeProviders,
}: ConnectionsTabProps) {
  return (
    <div className="provider-sections">
      <div>
        <h2 className="section-header">Device Code Providers</h2>
        <div className="provider-tree">
          {deviceCodeProviders.map((id) => {
            const C = DEVICE_CODE_COMPONENTS[id];
            return C ? (
              <div key={id} style={{ marginBottom: 12 }}>
                <C />
              </div>
            ) : null;
          })}
        </div>
      </div>

      <div>
        <h2 className="section-header">Multi-Account Providers</h2>
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {multiAccountProviders.map((p) => {
            const info = providerStatus?.providers[p];
            return (
              <ProviderCard
                key={p}
                provider={p}
                connected={info?.connected ?? false}
                email={info?.email}
                accounts={providerAccounts[p] ?? []}
                rateLimits={rateLimits}
                onRefresh={onRefresh}
              />
            );
          })}
        </div>
      </div>

      {isAdmin && (
        <div>
          <h2 className="section-header">OAuth Settings</h2>
          <OAuthSettings />
        </div>
      )}
    </div>
  );
}
