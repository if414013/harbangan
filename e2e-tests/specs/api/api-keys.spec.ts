import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

// API key lifecycle tests — must run serially (create → use → delete)
test.describe.configure({ mode: 'serial' });

test.describe('API Key Lifecycle', () => {
  let csrfToken: string;
  let createdKeyId: string;
  let createdKeyPlaintext: string;

  test.beforeAll(async ({ request }) => {
    // Clean up stale e2e test keys to avoid hitting the 10-key limit
    ({ csrfToken } = await adminLogin(request));
    const listRes = await request.get('/_ui/api/keys');
    const listBody = await listRes.json();
    const staleKeys = (listBody.keys ?? []).filter(
      (k: { label: string }) => k.label.startsWith('e2e-')
    );
    for (const key of staleKeys) {
      await request.delete(`/_ui/api/keys/${key.id}`, {
        headers: csrfHeaders(csrfToken),
      });
    }
  });

  test('create API key', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/keys', {
      data: { label: `e2e-test-${Date.now()}` },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();

    expect(body.id).toBeTruthy();
    expect(body.key).toBeTruthy();
    expect(body.key).toMatch(/^sk-/);
    expect(body.key_prefix).toBeTruthy();
    expect(body.label).toContain('e2e-test-');

    createdKeyId = body.id;
    createdKeyPlaintext = body.key;
  });

  test('list keys shows the new key', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/keys');
    expect(res.status()).toBe(200);
    const body = await res.json();

    expect(body).toHaveProperty('keys');
    expect(Array.isArray(body.keys)).toBe(true);

    const found = body.keys.find((k: { id: string }) => k.id === createdKeyId);
    expect(found).toBeTruthy();
    expect(found.key_prefix).toMatch(/^sk-/);
  });

  test('use key to authenticate GET /v1/models', async ({ request }) => {
    const res = await request.get('/v1/models', {
      headers: { 'Authorization': `Bearer ${createdKeyPlaintext}` },
    });
    // Should authenticate successfully — may return 200, 403 (no provider creds),
    // or 503 (setup mode), but should not be 401 (key invalid)
    expect([200, 403, 503]).toContain(res.status());
  });

  test('delete the key', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.delete(`/_ui/api/keys/${createdKeyId}`, {
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
  });

  test('deleted key returns 401 on /v1/models', async ({ request }) => {
    const res = await request.get('/v1/models', {
      headers: { 'Authorization': `Bearer ${createdKeyPlaintext}` },
    });
    expect(res.status()).toBe(401);
  });

  test('list keys no longer includes deleted key', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/keys');
    expect(res.status()).toBe(200);
    const body = await res.json();

    const found = body.keys.find((k: { id: string }) => k.id === createdKeyId);
    expect(found).toBeUndefined();
  });
});
