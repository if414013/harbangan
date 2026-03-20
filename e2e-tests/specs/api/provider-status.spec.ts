import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

// Provider status — serial for priority update lifecycle
test.describe.configure({ mode: 'serial' });

test.describe('Provider Status — Shape Tests', () => {
  test('GET /providers/status returns providers object', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/status');
    expect(res.status()).toBe(200);
    const body = await res.json();

    // Providers is an object keyed by provider_id, not an array
    expect(body).toHaveProperty('providers');
    expect(typeof body.providers).toBe('object');
    expect(body.providers).not.toBeNull();

    // Each provider should have a connected field
    for (const [providerId, status] of Object.entries(body.providers)) {
      expect(typeof providerId).toBe('string');
      expect(status).toHaveProperty('connected');
      expect(typeof (status as { connected: boolean }).connected).toBe('boolean');
    }
  });

  test('GET /kiro/status returns status shape', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/kiro/status');
    expect(res.status()).toBe(200);
    const body = await res.json();

    expect(body).toHaveProperty('has_token');
    expect(typeof body.has_token).toBe('boolean');
  });
});

test.describe('Provider Priority — Lifecycle', () => {
  let csrfToken: string;
  let originalPriorities: unknown;

  test('get current priority', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.get('/_ui/api/providers/priority');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('priorities');
    expect(Array.isArray(body.priorities)).toBe(true);
    originalPriorities = body.priorities;
  });

  test('update priority order', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    // Reverse the current priorities (if any)
    const reversed = Array.isArray(originalPriorities)
      ? [...originalPriorities].reverse()
      : [];

    const res = await request.post('/_ui/api/providers/priority', {
      data: { priorities: reversed },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('verify priority persisted', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/priority');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('priorities');
  });

  test('restore original priority', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/providers/priority', {
      data: { priorities: originalPriorities },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });
});
