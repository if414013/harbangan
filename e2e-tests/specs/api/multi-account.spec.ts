import { test, expect } from '@playwright/test';
import * as OTPAuth from 'otpauth';

// Multi-account tests modify shared state — run serially
test.describe.configure({ mode: 'serial' });

// ── Helpers ──────────────────────────────────────────────────────────

const ADMIN_EMAIL = process.env.INITIAL_ADMIN_EMAIL!;
const ADMIN_PASSWORD = process.env.INITIAL_ADMIN_PASSWORD!;
const TOTP_SECRET = process.env.INITIAL_ADMIN_TOTP_SECRET!;

function generateTOTP(): string {
  const totp = new OTPAuth.TOTP({
    issuer: 'KiroGateway',
    label: ADMIN_EMAIL,
    algorithm: 'SHA1',
    digits: 6,
    period: 30,
    secret: OTPAuth.Secret.fromBase32(TOTP_SECRET),
  });
  return totp.generate();
}

/** Login admin via password + 2FA and return the CSRF token. */
async function adminLogin(request: import('@playwright/test').APIRequestContext): Promise<{
  csrfToken: string;
}> {
  const loginRes = await request.post('/_ui/api/auth/login', {
    data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
  });
  expect(loginRes.status()).toBe(200);
  const loginBody = await loginRes.json();
  expect(loginBody.needs_2fa).toBe(true);

  const code = generateTOTP();
  const twoFaRes = await request.post('/_ui/api/auth/login/2fa', {
    data: { login_token: loginBody.login_token, code },
  });
  expect(twoFaRes.status()).toBe(200);

  const setCookie = twoFaRes.headers()['set-cookie'] ?? '';
  let csrfToken = '';
  const match = setCookie.match(/csrf_token=([^;]+)/);
  if (match) csrfToken = match[1];
  expect(csrfToken).toBeTruthy();

  return { csrfToken };
}

// ── Admin Pool CRUD ─────────────────────────────────────────────────

test.describe('Multi-Account — Admin Pool CRUD', () => {
  let csrfToken: string;
  let poolAccountId: string;

  test('POST /_ui/api/admin/pool — create a pool account', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/admin/pool', {
      data: {
        provider_id: 'anthropic',
        account_label: 'pool-test',
        api_key: 'sk-test-key-123',
      },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
  });

  test('GET /_ui/api/admin/pool — pool account appears in list', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/admin/pool');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('accounts');
    expect(Array.isArray(body.accounts)).toBe(true);

    const account = body.accounts.find(
      (a: { provider_id: string; account_label: string }) =>
        a.provider_id === 'anthropic' && a.account_label === 'pool-test'
    );
    expect(account).toBeTruthy();
    expect(account.enabled).toBe(true);
    expect(account.key_prefix).toBeTruthy();
    expect(account.id).toBeTruthy();

    // Save the ID for subsequent tests
    poolAccountId = account.id;
  });

  test('PATCH /_ui/api/admin/pool/:id/toggle — disable the account', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.patch(`/_ui/api/admin/pool/${poolAccountId}/toggle`, {
      data: { enabled: false },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
    expect(body.enabled).toBe(false);

    // Verify via GET
    const listRes = await request.get('/_ui/api/admin/pool');
    const listBody = await listRes.json();
    const account = listBody.accounts.find(
      (a: { id: string }) => a.id === poolAccountId
    );
    expect(account).toBeTruthy();
    expect(account.enabled).toBe(false);
  });

  test('DELETE /_ui/api/admin/pool/:id — remove pool account', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.delete(`/_ui/api/admin/pool/${poolAccountId}`, {
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);

    // Verify it's gone from the list
    const listRes = await request.get('/_ui/api/admin/pool');
    const listBody = await listRes.json();
    const account = listBody.accounts.find(
      (a: { id: string }) => a.id === poolAccountId
    );
    expect(account).toBeUndefined();
  });
});

// ── User Account Management ─────────────────────────────────────────

test.describe('Multi-Account — User Account Management', () => {
  test('GET /_ui/api/providers/anthropic/accounts — list user accounts', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/anthropic/accounts');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('accounts');
    expect(Array.isArray(body.accounts)).toBe(true);
  });

  test('DELETE /_ui/api/providers/anthropic/accounts/nonexistent — returns 200 (idempotent)', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.delete('/_ui/api/providers/anthropic/accounts/nonexistent', {
      headers: { 'x-csrf-token': csrfToken },
    });
    // Idempotent delete — should not error even if account doesn't exist
    expect(res.status()).toBe(200);
  });
});

// ── Rate Limit Monitoring ───────────────────────────────────────────

test.describe('Multi-Account — Rate Limit Monitoring', () => {
  test('GET /_ui/api/providers/rate-limits — returns accounts array', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/rate-limits');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('accounts');
    expect(Array.isArray(body.accounts)).toBe(true);

    // Each entry should have the expected shape
    for (const account of body.accounts) {
      expect(account).toHaveProperty('provider_id');
      expect(account).toHaveProperty('account_label');
      expect(typeof account.is_user_account).toBe('boolean');
      expect(typeof account.is_limited).toBe('boolean');
    }
  });
});

// ── Validation ──────────────────────────────────────────────────────

test.describe('Multi-Account — Admin Pool Validation', () => {
  test('POST /_ui/api/admin/pool with invalid provider_id — returns 400', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/admin/pool', {
      data: {
        provider_id: 'invalid-provider',
        account_label: 'bad-pool',
        api_key: 'sk-test-key-456',
      },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res.status()).toBe(400);
  });

  test('POST /_ui/api/admin/pool with missing api_key — returns 400', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/admin/pool', {
      data: {
        provider_id: 'anthropic',
        account_label: 'no-key-pool',
        // api_key intentionally omitted
      },
      headers: { 'x-csrf-token': csrfToken },
    });
    // Missing required field should fail deserialization → 400 (or 422)
    expect([400, 422]).toContain(res.status());
  });
});
