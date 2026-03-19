import { useState, useEffect } from "react";
import { PageHeader } from "../components/PageHeader";
import { TabBar } from "../components/TabBar";
import { useToast } from "../components/useToast";
import { useSession } from "../components/SessionGate";
import {
  getProvidersStatus,
  getRegistryModels,
  getUserProviderAccounts,
  getProviderRateLimits,
  updateModelEnabled,
  deleteRegistryModel,
  populateModels,
  apiFetch,
  getCopilotStatus,
  getQwenStatus,
} from "../lib/api";
import type {
  ProvidersStatusResponse,
  RegistryModel,
  UserProviderAccount,
  RateLimitInfo,
  KiroStatus,
} from "../lib/api";
import { StatusTab } from "./providers/StatusTab";
import { ConnectionsTab } from "./providers/ConnectionsTab";
import { ModelsTab } from "./providers/ModelsTab";

const MULTI_ACCOUNT_PROVIDERS = ["anthropic", "openai_codex"] as const;

const TABS = [
  { id: "status", label: "status" },
  { id: "connections", label: "connections" },
  { id: "models", label: "models" },
];

export function Providers() {
  const { showToast } = useToast();
  const { user } = useSession();
  const isAdmin = user.role === "admin";

  const [activeTab, setActiveTab] = useState("status");
  const [providerStatus, setProviderStatus] =
    useState<ProvidersStatusResponse | null>(null);
  const [models, setModels] = useState<RegistryModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(true);
  const [populating, setPopulating] = useState(false);
  const [providerAccounts, setProviderAccounts] = useState<
    Record<string, UserProviderAccount[]>
  >({});
  const [rateLimits, setRateLimits] = useState<RateLimitInfo[]>([]);
  const [kiroConnected, setKiroConnected] = useState(false);
  const [copilotConnected, setCopilotConnected] = useState(false);
  const [qwenConnected, setQwenConnected] = useState(false);
  const [confirmState, setConfirmState] = useState<{
    action: () => void;
    title: string;
    message: string;
  } | null>(null);

  function loadProviders() {
    getProvidersStatus()
      .then(setProviderStatus)
      .catch(() => {});
  }

  function loadModels() {
    getRegistryModels()
      .then((data) => {
        setModels(data.models);
        setModelsLoading(false);
      })
      .catch(() => {
        setModelsLoading(false);
      });
  }

  function loadAccounts() {
    for (const p of MULTI_ACCOUNT_PROVIDERS) {
      getUserProviderAccounts(p)
        .then((data) => {
          setProviderAccounts((prev) => ({ ...prev, [p]: data.accounts }));
        })
        .catch(() => {});
    }
  }

  function loadRateLimits() {
    getProviderRateLimits()
      .then((data) => setRateLimits(data.accounts))
      .catch(() => {});
  }

  function loadDeviceCodeStatuses() {
    apiFetch<KiroStatus>("/kiro/status")
      .then((s) => setKiroConnected(s.has_token && !s.expired))
      .catch(() => {});
    getCopilotStatus()
      .then((s) => setCopilotConnected(s.connected && !s.expired))
      .catch(() => {});
    getQwenStatus()
      .then((s) => setQwenConnected(s.connected && !s.expired))
      .catch(() => {});
  }

  function refreshAll() {
    loadProviders();
    loadAccounts();
    loadRateLimits();
    loadDeviceCodeStatuses();
  }

  useEffect(() => {
    loadProviders();
    loadModels();
    loadAccounts();
    loadRateLimits();
    loadDeviceCodeStatuses();
  }, []);

  async function handleToggle(id: string, enabled: boolean) {
    try {
      await updateModelEnabled(id, enabled);
      setModels((prev) =>
        prev.map((m) => (m.id === id ? { ...m, enabled } : m)),
      );
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to update model",
        "error",
      );
    }
  }

  function handleDelete(id: string) {
    setConfirmState({
      action: async () => {
        try {
          await deleteRegistryModel(id);
          showToast("Model deleted", "success");
          setModels((prev) => prev.filter((m) => m.id !== id));
        } catch (err) {
          showToast(
            err instanceof Error ? err.message : "Failed to delete model",
            "error",
          );
        }
      },
      title: "Delete model",
      message: "Delete this model from the registry?",
    });
  }

  async function handlePopulate(providerId?: string) {
    setPopulating(true);
    try {
      const res = await populateModels(providerId);
      showToast(`Populated ${res.models_upserted} models`, "success");
      loadModels();
    } catch (err) {
      showToast(
        err instanceof Error ? err.message : "Failed to populate models",
        "error",
      );
    } finally {
      setPopulating(false);
    }
  }

  function handleNavigateToConnections() {
    setActiveTab("connections");
  }

  return (
    <>
      <PageHeader
        title="providers"
        description="Connect provider accounts and manage model access."
      />
      <TabBar tabs={TABS} activeTab={activeTab} onTabChange={setActiveTab} />
      {activeTab === "status" && (
        <StatusTab
          providerStatus={providerStatus}
          models={models}
          providerAccounts={providerAccounts}
          rateLimits={rateLimits}
          kiroConnected={kiroConnected}
          copilotConnected={copilotConnected}
          qwenConnected={qwenConnected}
          onNavigate={handleNavigateToConnections}
        />
      )}
      {activeTab === "connections" && (
        <ConnectionsTab
          providerStatus={providerStatus}
          providerAccounts={providerAccounts}
          rateLimits={rateLimits}
          isAdmin={isAdmin}
          onRefresh={refreshAll}
        />
      )}
      {activeTab === "models" && (
        <ModelsTab
          models={models}
          modelsLoading={modelsLoading}
          populating={populating}
          confirmState={confirmState}
          onToggle={handleToggle}
          onDelete={handleDelete}
          onPopulate={handlePopulate}
          onConfirm={() => {
            confirmState?.action();
            setConfirmState(null);
          }}
          onCancelConfirm={() => setConfirmState(null)}
        />
      )}
    </>
  );
}
