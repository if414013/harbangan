import { apiFetch } from "./api";

export interface UsageRecord {
  group_key: string;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost: number;
  request_count: number;
}

export interface UserUsageRecord {
  user_id: string;
  email: string;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost: number;
  request_count: number;
}

interface UsageResponseWrapper {
  start_date: string;
  end_date: string;
  group_by: string;
  data: UsageRecord[];
}

interface UserUsageResponseWrapper {
  start_date: string;
  end_date: string;
  data: UserUsageRecord[];
}

export async function fetchUsage(params: {
  start_date?: string;
  end_date?: string;
  group_by?: string;
}): Promise<UsageRecord[]> {
  const searchParams = new URLSearchParams();
  if (params.start_date) searchParams.set("start_date", params.start_date);
  if (params.end_date) searchParams.set("end_date", params.end_date);
  if (params.group_by) searchParams.set("group_by", params.group_by);
  const res = await apiFetch<UsageResponseWrapper>(
    `/usage?${searchParams.toString()}`,
  );
  return res.data;
}

export async function fetchAdminUsage(params: {
  start_date?: string;
  end_date?: string;
  group_by?: string;
}): Promise<UsageRecord[]> {
  const searchParams = new URLSearchParams();
  if (params.start_date) searchParams.set("start_date", params.start_date);
  if (params.end_date) searchParams.set("end_date", params.end_date);
  if (params.group_by) searchParams.set("group_by", params.group_by);
  const res = await apiFetch<UsageResponseWrapper>(
    `/admin/usage?${searchParams.toString()}`,
  );
  return res.data;
}

export async function fetchAdminUsageByUsers(params: {
  start_date?: string;
  end_date?: string;
}): Promise<UserUsageRecord[]> {
  const searchParams = new URLSearchParams();
  if (params.start_date) searchParams.set("start_date", params.start_date);
  if (params.end_date) searchParams.set("end_date", params.end_date);
  const res = await apiFetch<UserUsageResponseWrapper>(
    `/admin/usage/users?${searchParams.toString()}`,
  );
  return res.data;
}
