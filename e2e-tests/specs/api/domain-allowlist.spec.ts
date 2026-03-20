import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';
import * as OTPAuth from 'otpauth';

// Domain allowlist tests modify shared state — run serially
test.describe.configure({ mode: 'serial' });

const ADMIN_EMAIL = process.env.INITIAL_ADMIN_EMAIL!;
const ADMIN_PASSWORD = process.env.INITIAL_ADMIN_PASSWORD!;
const TOTP_SECRET = process.env.INITIAL_ADMIN_TOTP_SECRET!;

const TEST_DOMAIN = `test-e2e-${Date.now()}.example.com`;

test.describe('Domain Allowlist — CRUD', () => {
  let csrfToken: string;

  test('list domains returns array', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.get('/_ui/api/domains');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('domains');
    expect(Array.isArray(body.domains)).toBe(true);
    expect(body).toHaveProperty('count');
    expect(typeof body.count).toBe('number');
  });

  test('add domain', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/domains', {
      data: { domain: TEST_DOMAIN },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
  });

  test('added domain appears in list', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/domains');
    expect(res.status()).toBe(200);
    const body = await res.json();

    const found = body.domains.find(
      (d: { domain: string }) => d.domain === TEST_DOMAIN.toLowerCase()
    );
    expect(found).toBeTruthy();
    expect(found.created_at).toBeTruthy();
  });

  test('remove domain', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.delete(`/_ui/api/domains/${TEST_DOMAIN}`, {
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
  });

  test('removed domain no longer in list', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/domains');
    expect(res.status()).toBe(200);
    const body = await res.json();

    const found = body.domains.find(
      (d: { domain: string }) => d.domain === TEST_DOMAIN.toLowerCase()
    );
    expect(found).toBeUndefined();
  });
});

test.describe('Domain Allowlist — Validation', () => {
  test('empty domain rejected', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/domains', {
      data: { domain: '' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(400);
  });

  test('domain with invalid characters rejected', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/domains', {
      data: { domain: 'bad domain with spaces.com' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(400);
  });
});

test.describe('Domain Allowlist — RBAC', () => {
  test('non-admin cannot list domains', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Create a regular user
    const userEmail = `domain-rbac-${Date.now()}@example.com`;
    const createRes = await request.post('/_ui/api/admin/users/create', {
      data: { email: userEmail, name: 'Domain RBAC Test', password: 'DomainRbac123!', role: 'user' },
      headers: csrfHeaders(csrfToken),
    });
    expect(createRes.status()).toBe(200);

    // Login as regular user (no TOTP → direct session)
    const userLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: userEmail, password: 'DomainRbac123!' },
    });
    expect(userLoginRes.status()).toBe(200);
    const userCookies = userLoginRes.headers()['set-cookie'] ?? '';
    const csrfMatch = userCookies.match(/csrf_token=([^;]+)/);
    const userCsrf = csrfMatch ? csrfMatch[1] : '';

    // Non-admin: GET domains → 403
    const listRes = await request.get('/_ui/api/domains');
    expect(listRes.status()).toBe(403);

    // Non-admin: POST domain → 403
    const addRes = await request.post('/_ui/api/domains', {
      data: { domain: 'rbac-test.example.com' },
      headers: csrfHeaders(userCsrf),
    });
    expect(addRes.status()).toBe(403);

    // Non-admin: DELETE domain → 403
    const delRes = await request.delete('/_ui/api/domains/rbac-test.example.com', {
      headers: csrfHeaders(userCsrf),
    });
    expect(delRes.status()).toBe(403);
  });
});
