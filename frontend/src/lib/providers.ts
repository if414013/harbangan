import type { RegistryModel } from "./api";

const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  openai_codex: "OpenAI Codex",
};

export function providerDisplayName(id: string): string {
  return PROVIDER_DISPLAY_NAMES[id] ?? id.charAt(0).toUpperCase() + id.slice(1);
}

export interface ProviderGroup {
  providerId: string;
  models: RegistryModel[];
}

export function groupByProvider(models: RegistryModel[]): ProviderGroup[] {
  const map = new Map<string, RegistryModel[]>();
  for (const m of models) {
    const list = map.get(m.provider_id) ?? [];
    list.push(m);
    map.set(m.provider_id, list);
  }
  return Array.from(map.entries())
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([providerId, models]) => ({ providerId, models }));
}
