import { useState } from "react";
import { ConfirmDialog } from "./ConfirmDialog";
import { RelayModal } from "./RelayModal";
import { useToast } from "./useToast";
import {
  getProviderConnectUrl,
  disconnectProvider,
  deleteUserProviderAccount,
} from "../lib/api";
import type { UserProviderAccount, RateLimitInfo } from "../lib/api";
import { providerDisplayName } from "../lib/providers";

interface ProviderCardProps {
  provider: string;
  connected: boolean;
  email?: string;
  accounts: UserProviderAccount[];
  rateLimits: RateLimitInfo[];
  onRefresh: () => void;
}

export function ProviderCard({
  provider,
  connected,
  email,
  accounts,
  rateLimits,
  onRefresh,
}: ProviderCardProps) {
  const { showToast } = useToast();
  const [connecting, setConnecting] = useState(false);
  const [relayUrl, setRelayUrl] = useState<string | null>(null);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  async function handleConnect() {
    setConnecting(true);
    try {
      const result = await getProviderConnectUrl(provider);
      setRelayUrl(result.relay_script_url);
    } catch (err) {
      showToast(
        "Failed to start connect: " +
          (err instanceof Error ? err.message : "Unknown error"),
        "error",
      );
    } finally {
      setConnecting(false);
    }
  }

  async function handleDisconnect() {
    try {
      await disconnectProvider(provider);
      showToast(`${provider} disconnected`, "success");
      onRefresh();
    } catch (err) {
      showToast(
        "Failed to disconnect: " +
          (err instanceof Error ? err.message : "Unknown error"),
        "error",
      );
    }
  }

  function handleDeleteAccount(label: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteUserProviderAccount(provider, label);
          showToast(`Account "${label}" removed`, "success");
          onRefresh();
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to remove account",
            "error",
          );
        }
      },
      title: "Remove account",
      message: `Remove account "${label}" from ${providerDisplayName(provider)}?`,
    });
  }

  function handleConnected() {
    setRelayUrl(null);
    showToast(`${provider} connected`, "success");
    onRefresh();
  }

  function getRateLimit(label: string): RateLimitInfo | undefined {
    return rateLimits.find(
      (r) => r.provider_id === provider && r.account_label === label,
    );
  }

  return (
    <>
      <div className="card provider-card">
        <div className="card-header">
          <span className="card-title">
            {"> "}
            {providerDisplayName(provider)}
          </span>
          {connected ? (
            <span className="tag-ok">CONNECTED</span>
          ) : (
            <span className="tag-err">NOT CONNECTED</span>
          )}
        </div>
        {connected && email && accounts.length === 0 && (
          <div className="provider-email">{email}</div>
        )}

        {accounts.length > 0 && (
          <div className="account-list">
            {accounts.map((acct) => {
              const rl = getRateLimit(acct.account_label);
              const isLimited = rl?.limited_until != null;
              return (
                <div key={acct.account_label} className="account-row">
                  <div className="account-row-info">
                    <span className="account-label">{acct.account_label}</span>
                    {acct.email && (
                      <span className="account-email">{acct.email}</span>
                    )}
                    {isLimited && (
                      <span className="tag-warn">RATE LIMITED</span>
                    )}
                    {rl && rl.requests_remaining != null && !isLimited && (
                      <span className="account-rate">
                        {rl.requests_remaining} req left
                      </span>
                    )}
                  </div>
                  <button
                    className="btn-danger"
                    type="button"
                    onClick={() => handleDeleteAccount(acct.account_label)}
                  >
                    remove
                  </button>
                </div>
              );
            })}
          </div>
        )}

        <div className="kiro-actions">
          {connected ? (
            <>
              <button
                className="btn-save"
                type="button"
                onClick={handleConnect}
                disabled={connecting}
              >
                {connecting ? "..." : "$ connect another"}
              </button>
              <button
                className="btn-danger"
                type="button"
                onClick={handleDisconnect}
              >
                $ disconnect all
              </button>
            </>
          ) : (
            <button
              className="btn-save"
              type="button"
              onClick={handleConnect}
              disabled={connecting}
            >
              {connecting ? "..." : "$ connect"}
            </button>
          )}
        </div>
      </div>
      {relayUrl && (
        <RelayModal
          provider={provider}
          relayScriptUrl={relayUrl}
          onConnected={handleConnected}
          onClose={() => setRelayUrl(null)}
        />
      )}
      {confirmState && (
        <ConfirmDialog
          title={confirmState.title}
          message={confirmState.message}
          confirmLabel="Remove"
          variant="danger"
          onConfirm={() => {
            confirmState.action();
            setConfirmState(null);
          }}
          onCancel={() => setConfirmState(null)}
        />
      )}
    </>
  );
}
