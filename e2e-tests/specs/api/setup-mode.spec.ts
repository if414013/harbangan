import { test, expect } from '@playwright/test';

/**
 * Setup-only mode tests.
 *
 * Run via: npm run test:setup-mode
 * Uses playwright.setup-mode.config.ts (no globalSetup, no admin seeding).
 * Requires a backend with an empty database (no admin user created).
 *
 * These tests verify that an unconfigured gateway:
 * 1. Reports setup_complete: false
 * 2. Blocks proxy requests with 503
 * 3. Still serves the web UI status endpoint
 *
 * When running in the normal test suite (admin already seeded),
 * these tests are automatically skipped.
 */

test.describe('Setup-Only Mode', () => {
  test.beforeEach(async ({ request }) => {
    const res = await request.get('/_ui/api/status');
    const body = await res.json();
    test.skip(body.setup_complete === true, 'Gateway already set up — setup mode not active');
  });

  test('GET /status reports setup_complete: false', async ({ request }) => {
    const res = await request.get('/_ui/api/status');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.setup_complete).toBe(false);
  });

  test('POST /v1/chat/completions returns 503', async ({ request }) => {
    const res = await request.post('/v1/chat/completions', {
      data: {
        model: 'test-model',
        messages: [{ role: 'user', content: 'test' }],
      },
      headers: { 'Content-Type': 'application/json' },
    });
    expect(res.status()).toBe(503);
  });

  test('GET /v1/models returns 503', async ({ request }) => {
    const res = await request.get('/v1/models');
    expect(res.status()).toBe(503);
  });

  test('GET /health still responds', async ({ request }) => {
    const res = await request.get('/health');
    expect(res.status()).toBe(200);
  });
});
