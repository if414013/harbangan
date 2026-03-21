# Fix: Usage Page Empty Due to Response Format Mismatch

## Context

The Usage page at `/_ui/usage` renders empty because the frontend API client returns the full backend response object instead of extracting the `.data` array. The backend wraps usage data in `UsageResponse { start_date, end_date, group_by, data }` and `UserUsageResponse { start_date, end_date, data }`, but the frontend treats the response as a plain array.

## Fix

**File:** `frontend/src/lib/api_usage.ts`

All three functions need to unwrap `.data` from the response:

1. `fetchUsage()` (line 29): Add response type interface, extract `.data`
2. `fetchAdminUsage()` (line 41): Same fix
3. `fetchAdminUsageByUsers()` (line 51): Same fix, uses `UserUsageResponse` shape

Add two response interfaces matching the backend:
```typescript
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
```

Then change each function to:
```typescript
const response = await apiFetch<UsageResponseWrapper>(`/usage?${searchParams.toString()}`);
return response.data;
```

## Verification

1. `cd frontend && npm run build` — confirm no type errors
2. `cd frontend && npm run lint` — confirm no lint errors
3. Open `/_ui/usage` in Playwright and verify data renders (or at least no empty page with "0" values)
