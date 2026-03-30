import { test, expect, request as playwrightRequest } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

// Provider toggle — serial lifecycle (mutates shared state)
test.describe.configure({ mode: 'serial' });

test.describe('Provider Toggle — Admin Lifecycle', () => {
  test('admin can disable a provider', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.patch('/_ui/api/admin/providers/anthropic', {
      data: { enabled: false },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('disabled provider reflected in registry', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/registry');
    expect(res.status()).toBe(200);
    const body = await res.json();
    const anthropic = body.providers.find(
      (p: { id: string }) => p.id === 'anthropic'
    );
    expect(anthropic).toBeTruthy();
    expect(anthropic.enabled).toBe(false);
  });

  test('admin can re-enable a provider', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.patch('/_ui/api/admin/providers/anthropic', {
      data: { enabled: true },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('re-enabled provider reflected in registry', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/registry');
    expect(res.status()).toBe(200);
    const body = await res.json();
    const anthropic = body.providers.find(
      (p: { id: string }) => p.id === 'anthropic'
    );
    expect(anthropic).toBeTruthy();
    expect(anthropic.enabled).toBe(true);
  });
});

test.describe('Provider Toggle — Kiro Protection', () => {
  test('disabling kiro is rejected with 400', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.patch('/_ui/api/admin/providers/kiro', {
      data: { enabled: false },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(400);
  });
});

test.describe('Provider Toggle — Non-Admin Rejection', () => {
  test('unauthenticated request gets 401', async () => {
    const ctx = await playwrightRequest.newContext({
      baseURL: process.env.GATEWAY_URL || 'http://localhost:9999',
    });

    const res = await ctx.patch('/_ui/api/admin/providers/anthropic', {
      data: { enabled: false },
    });
    expect(res.status()).toBe(401);
    await ctx.dispose();
  });
});
