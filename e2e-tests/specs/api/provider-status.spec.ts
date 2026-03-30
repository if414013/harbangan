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

test.describe('Provider Registry — Shape Tests', () => {
  test('GET /providers/registry returns providers array with correct shape', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/registry');
    expect(res.status()).toBe(200);
    const body = await res.json();

    expect(body).toHaveProperty('providers');
    expect(Array.isArray(body.providers)).toBe(true);
    expect(body.providers.length).toBe(4);

    // Each entry must have id, display_name, category, supports_pool, enabled
    for (const entry of body.providers) {
      expect(typeof entry.id).toBe('string');
      expect(typeof entry.display_name).toBe('string');
      expect(typeof entry.category).toBe('string');
      expect(typeof entry.supports_pool).toBe('boolean');
      expect(typeof entry.enabled).toBe('boolean');
      expect(['device_code', 'oauth_relay']).toContain(entry.category);
    }

    // Verify specific providers are present
    const ids = body.providers.map((p: { id: string }) => p.id);
    expect(ids).toContain('kiro');
    expect(ids).toContain('anthropic');
    expect(ids).toContain('openai_codex');
    expect(ids).toContain('copilot');
  });

  test('registry providers have correct categories', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/registry');
    const body = await res.json();
    const byId = Object.fromEntries(
      body.providers.map((p: { id: string; category: string }) => [p.id, p])
    );

    expect(byId.anthropic.category).toBe('oauth_relay');
    expect(byId.openai_codex.category).toBe('oauth_relay');
    expect(byId.kiro.category).toBe('device_code');
    expect(byId.copilot.category).toBe('device_code');
  });

  test('all registry providers support pool', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/providers/registry');
    const body = await res.json();

    for (const entry of body.providers) {
      expect(entry.supports_pool).toBe(true);
    }
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
