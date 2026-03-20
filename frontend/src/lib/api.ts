import { authHeaders } from "./auth";

const BASE = "/_ui/api";

export class ApiResponseError extends Error {
  readonly status: number;
  readonly code: string | undefined;

  constructor(status: number, message: string, code?: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export async function apiFetch<T>(
  path: string,
  init?: RequestInit,
): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    credentials: "include",
    headers: { ...authHeaders(), ...init?.headers },
  });

  if (res.status === 401) {
    window.location.href = "/_ui/login";
    throw new ApiResponseError(401, "Session expired");
  }

  if (!res.ok) {
    let body:
      | {
          error?: string | { message?: string; type?: string };
          message?: string;
        }
      | undefined;
    try {
      body = await res.json();
    } catch {
      /* not JSON */
    }
    const errObj = body?.error;
    const msg =
      body?.message ||
      (typeof errObj === "object" && errObj?.message) ||
      (typeof errObj === "string" && errObj) ||
      `HTTP ${res.status}`;
    const code =
      typeof errObj === "object"
        ? errObj?.type
        : typeof errObj === "string"
          ? errObj
          : undefined;
    throw new ApiResponseError(res.status, msg, code);
  }

  if (res.status === 204) return undefined as T;
  const text = await res.text();
  return text ? (JSON.parse(text) as T) : (undefined as T);
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: "POST",
    headers: body !== undefined ? { "Content-Type": "application/json" } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
}

export async function apiDelete(path: string): Promise<void> {
  const res = await fetch(`${BASE}${path}`, {
    method: "DELETE",
    credentials: "include",
    headers: authHeaders(),
  });
  if (res.status === 401) {
    window.location.href = "/_ui/login";
    throw new ApiResponseError(401, "Session expired");
  }
  if (!res.ok) {
    let body:
      | {
          error?: string | { message?: string; type?: string };
          message?: string;
        }
      | undefined;
    try {
      body = await res.json();
    } catch {
      /* not JSON */
    }
    const errObj = body?.error;
    const msg =
      body?.message ||
      (typeof errObj === "object" && errObj?.message) ||
      (typeof errObj === "string" && errObj) ||
      `HTTP ${res.status}`;
    const code =
      typeof errObj === "object"
        ? errObj?.type
        : typeof errObj === "string"
          ? errObj
          : undefined;
    throw new ApiResponseError(res.status, msg, code);
  }
}

export async function checkSetupStatus(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE}/status`, { credentials: "include" });
    if (!res.ok) return false;
    const data = await res.json();
    return data.setup_complete === true;
  } catch {
    return false;
  }
}

export async function pollDeviceCode(
  deviceCode: string,
): Promise<DevicePollResponse> {
  return apiPost<DevicePollResponse>("/kiro/poll", { device_code: deviceCode });
}

// --- Types ---

export interface User {
  id: string;
  email: string;
  name: string;
  picture_url: string | null;
  role: "admin" | "user";
  last_login: string | null;
  created_at: string;
  auth_method?: "google" | "password";
  totp_enabled?: boolean;
  must_change_password?: boolean;
  google_linked?: boolean;
}

export interface LoginResponse {
  needs_2fa: boolean;
  login_token?: string;
}

export interface TotpSetupResponse {
  secret: string;
  qr_code_data_url: string;
  recovery_codes: string[];
}

export interface StatusResponse {
  setup_complete: boolean;
  google_configured: boolean;
  auth_google_enabled: boolean;
  auth_password_enabled: boolean;
}

export interface ApiKeyInfo {
  id: string;
  key_prefix: string;
  label: string;
  last_used: string | null;
  created_at: string;
}

export interface ApiKeyCreateResponse {
  id: string;
  key: string;
  key_prefix: string;
  label: string;
}

export interface KiroStatus {
  has_token: boolean;
  expired: boolean;
  sso_start_url?: string;
  sso_region?: string;
}

export interface DeviceCodeResponse {
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string;
  device_code: string;
}

export interface DevicePollResponse {
  status: "pending" | "slow_down" | "success" | "expired" | "denied";
  message?: string;
}

export interface DomainInfo {
  domain: string;
  added_by: string | null;
  created_at: string;
}

export interface UserDetailResponse {
  user: User;
  api_keys: ApiKeyInfo[];
  kiro_status: KiroStatus;
}

// --- Guardrails Types ---

export interface GuardrailProfile {
  id: string;
  name: string;
  provider_name: string;
  enabled: boolean;
  guardrail_id: string;
  guardrail_version: string;
  region: string;
  access_key: string;
  secret_key: string;
  created_at: string;
  updated_at: string;
}

export interface GuardrailRule {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  cel_expression: string;
  apply_to: "input" | "output" | "both";
  sampling_rate: number;
  timeout_ms: number;
  profile_ids: string[];
  created_at: string;
  updated_at: string;
}

export interface CelValidationResult {
  valid: boolean;
  error?: string;
}

export interface GuardrailTestResult {
  success: boolean;
  action: string;
  response_time_ms: number;
  error?: string;
}

// --- Guardrails API ---

export function getGuardrailProfiles() {
  return apiFetch<GuardrailProfile[]>("/admin/guardrails/profiles");
}

export function createGuardrailProfile(profile: Partial<GuardrailProfile>) {
  return apiPost<GuardrailProfile>("/admin/guardrails/profiles", profile);
}

export function updateGuardrailProfile(
  id: string,
  profile: Partial<GuardrailProfile>,
) {
  return apiPut<GuardrailProfile>(`/admin/guardrails/profiles/${id}`, profile);
}

export function deleteGuardrailProfile(id: string) {
  return apiDelete(`/admin/guardrails/profiles/${id}`);
}

export function getGuardrailRules() {
  return apiFetch<GuardrailRule[]>("/admin/guardrails/rules");
}

export function createGuardrailRule(rule: Partial<GuardrailRule>) {
  return apiPost<GuardrailRule>("/admin/guardrails/rules", rule);
}

export function updateGuardrailRule(id: string, rule: Partial<GuardrailRule>) {
  return apiPut<GuardrailRule>(`/admin/guardrails/rules/${id}`, rule);
}

export function deleteGuardrailRule(id: string) {
  return apiDelete(`/admin/guardrails/rules/${id}`);
}

export function testGuardrailProfile(profileId: string, content: string) {
  return apiPost<GuardrailTestResult>("/admin/guardrails/test", {
    profile_id: profileId,
    content,
  });
}

export function validateCelExpression(expression: string) {
  return apiPost<CelValidationResult>("/admin/guardrails/cel/validate", {
    expression,
  });
}

// --- Provider Registry Types ---

export interface ProviderRegistryEntry {
  id: string;
  display_name: string;
  category: "device_code" | "oauth_relay" | "custom";
  supports_pool: boolean;
}

export function getProviderRegistry() {
  return apiFetch<{ providers: ProviderRegistryEntry[] }>(
    "/providers/registry",
  );
}

// --- Provider OAuth Types ---

export interface ProviderStatus {
  connected: boolean;
  email?: string;
}

export interface ProvidersStatusResponse {
  providers: Record<string, ProviderStatus>;
}

export interface ProviderConnectResponse {
  relay_script_url: string;
}

// --- Provider OAuth API ---

export function getProvidersStatus() {
  return apiFetch<ProvidersStatusResponse>("/providers/status");
}

export function getProviderConnectUrl(provider: string) {
  return apiFetch<ProviderConnectResponse>(`/providers/${provider}/connect`);
}

export function disconnectProvider(provider: string) {
  return apiDelete(`/providers/${provider}`);
}

// --- Multi-Account Types ---

export interface AdminPoolAccount {
  id: string;
  provider_id: string;
  account_label: string;
  key_prefix: string;
  base_url: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface UserProviderAccount {
  provider_id: string;
  account_label: string;
  email: string | null;
  base_url: string | null;
  created_at: string;
}

export interface RateLimitInfo {
  provider_id: string;
  account_label: string;
  requests_remaining: number | null;
  tokens_remaining: number | null;
  limited_until: string | null;
  updated_at: string | null;
}

// --- Admin Pool API ---

export function getAdminPoolAccounts() {
  return apiFetch<{ accounts: AdminPoolAccount[] }>("/admin/pool");
}

export function addAdminPoolAccount(data: {
  provider_id: string;
  account_label: string;
  api_key: string;
  key_prefix?: string;
  base_url?: string;
}) {
  return apiPost<void>("/admin/pool", data);
}

export function deleteAdminPoolAccount(id: string) {
  return apiDelete(`/admin/pool/${id}`);
}

export function toggleAdminPoolAccount(id: string, enabled: boolean) {
  return apiFetch<void>(`/admin/pool/${id}/toggle`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled }),
  });
}

// --- User Provider Accounts API ---

export function getUserProviderAccounts(provider: string) {
  return apiFetch<{ accounts: UserProviderAccount[] }>(
    `/providers/${provider}/accounts`,
  );
}

export function deleteUserProviderAccount(provider: string, label: string) {
  return apiDelete(
    `/providers/${provider}/accounts/${encodeURIComponent(label)}`,
  );
}

// --- Rate Limits API ---

export function getProviderRateLimits() {
  return apiFetch<{ accounts: RateLimitInfo[] }>("/providers/rate-limits");
}

// --- Copilot Types ---

export interface CopilotStatus {
  connected: boolean;
  github_username: string | null;
  copilot_plan: string | null;
  expired: boolean;
}

export interface CopilotDeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

// --- Copilot API ---

export function getCopilotStatus() {
  return apiFetch<CopilotStatus>("/copilot/status");
}

export function startCopilotDeviceFlow() {
  return apiPost<CopilotDeviceCodeResponse>("/copilot/device-code");
}

export function pollCopilotDeviceCode(deviceCode: string) {
  return apiFetch<DevicePollResponse>(
    `/copilot/device-poll?device_code=${encodeURIComponent(deviceCode)}`,
  );
}

export function disconnectCopilot() {
  return apiDelete("/copilot/disconnect");
}

// --- Qwen Types ---

export interface QwenStatus {
  connected: boolean;
  expired: boolean;
}

export interface QwenDeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete?: string;
  expires_in: number;
  interval: number;
}

// --- Qwen API ---

export function getQwenStatus() {
  return apiFetch<QwenStatus>("/providers/qwen/status");
}

export function startQwenDeviceFlow() {
  return apiPost<QwenDeviceCodeResponse>("/providers/qwen/device-code");
}

export function pollQwenDeviceCode(deviceCode: string) {
  return apiFetch<DevicePollResponse>(
    `/providers/qwen/device-poll?device_code=${encodeURIComponent(deviceCode)}`,
  );
}

export function disconnectQwen() {
  return apiDelete("/providers/qwen/disconnect");
}

// --- Auth (Password + 2FA) API ---

export function getStatus() {
  return apiFetch<StatusResponse>("/status");
}

export function loginWithPassword(email: string, password: string) {
  return apiPost<LoginResponse>("/auth/login", { email, password });
}

export function verify2FA(loginToken: string, code: string) {
  return apiPost<void>("/auth/login/2fa", { login_token: loginToken, code });
}

export function getTotpSetup() {
  return apiFetch<TotpSetupResponse>("/auth/2fa/setup");
}

export function verifyTotpSetup(code: string) {
  return apiPost<void>("/auth/2fa/verify", { code });
}

export function changePassword(currentPassword: string, newPassword: string) {
  return apiPost<void>("/auth/password/change", {
    current_password: currentPassword,
    new_password: newPassword,
  });
}

export function adminCreateUser(
  email: string,
  name: string,
  password: string,
  role: "admin" | "user",
) {
  return apiPost<User>("/admin/users/create", { email, name, password, role });
}

export function adminResetPassword(userId: string, newPassword: string) {
  return apiPost<void>(`/admin/users/${userId}/reset-password`, {
    new_password: newPassword,
  });
}

// --- Model Registry Types ---

export interface RegistryModel {
  id: string;
  provider_id: string;
  model_id: string;
  display_name: string;
  prefixed_id: string;
  context_length: number;
  max_output_tokens: number;
  capabilities: Record<string, unknown>;
  enabled: boolean;
  source: string;
  upstream_meta: Record<string, unknown> | null;
  created_at: string;
  updated_at: string;
}

export interface ModelsListResponse {
  models: RegistryModel[];
  total: number;
}

export interface UpdateModelResponse {
  success: boolean;
  id: string;
}

export interface PopulateResponse {
  success: boolean;
  models_upserted: number;
}

export interface DeleteModelResponse {
  success: boolean;
  id: string;
}

// --- Model Registry API ---

export function getRegistryModels() {
  return apiFetch<ModelsListResponse>("/models/registry");
}

export function updateModelEnabled(id: string, enabled: boolean) {
  return apiFetch<UpdateModelResponse>(`/models/registry/${id}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled }),
  });
}

export function deleteRegistryModel(id: string) {
  return apiDelete(`/models/registry/${id}`);
}

export function populateModels(providerId?: string) {
  return apiPost<PopulateResponse>("/models/registry/populate", {
    provider_id: providerId ?? null,
  });
}
