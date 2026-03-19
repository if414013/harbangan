import { KiroSetup } from "../../components/KiroSetup";
import { CopilotSetup } from "../../components/CopilotSetup";
import { QwenSetup } from "../../components/QwenSetup";
import { ProviderCard } from "../../components/ProviderCard";
import { OAuthSettings } from "../../components/OAuthSettings";
import type {
  ProvidersStatusResponse,
  UserProviderAccount,
  RateLimitInfo,
} from "../../lib/api";

const MULTI_ACCOUNT_PROVIDERS = ["anthropic", "openai_codex"] as const;

interface ConnectionsTabProps {
  providerStatus: ProvidersStatusResponse | null;
  providerAccounts: Record<string, UserProviderAccount[]>;
  rateLimits: RateLimitInfo[];
  isAdmin: boolean;
  onRefresh: () => void;
}

export function ConnectionsTab({
  providerStatus,
  providerAccounts,
  rateLimits,
  isAdmin,
  onRefresh,
}: ConnectionsTabProps) {
  return (
    <div className="provider-sections">
      <div>
        <h2 className="section-header">Device Code Providers</h2>
        <div className="provider-tree">
          <div style={{ marginBottom: 12 }}>
            <KiroSetup />
          </div>
          <div style={{ marginBottom: 12 }}>
            <CopilotSetup />
          </div>
          <div style={{ marginBottom: 12 }}>
            <QwenSetup />
          </div>
        </div>
      </div>

      <div>
        <h2 className="section-header">Multi-Account Providers</h2>
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {MULTI_ACCOUNT_PROVIDERS.map((p) => {
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
