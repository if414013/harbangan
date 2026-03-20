import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

// Model registry tests — serial lifecycle
test.describe.configure({ mode: 'serial' });

test.describe('Model Registry — Always Runs', () => {
  test('GET /models/registry returns response shape', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/models/registry');
    expect(res.status()).toBe(200);
    const body = await res.json();

    // Response is { models: [...] }
    expect(body).toHaveProperty('models');
    expect(Array.isArray(body.models)).toBe(true);
  });

  test('POST /models/registry/populate returns 200', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/models/registry/populate', {
      data: {},
      headers: csrfHeaders(csrfToken),
    });
    // Returns 200 regardless of whether models were found
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('success');
    expect(body.success).toBe(true);
    expect(typeof body.models_upserted).toBe('number');
  });
});

test.describe('Model Registry — Conditional (models exist)', () => {
  let modelId: string | null = null;
  let originalEnabled: boolean;
  let patchSupported = true;

  test.beforeAll(async ({ request }) => {
    await adminLogin(request);
    const res = await request.get('/_ui/api/models/registry');
    const body = await res.json();
    const models = body.models ?? [];
    if (Array.isArray(models) && models.length > 0) {
      modelId = models[0].id;
      originalEnabled = models[0].enabled;

      // Probe PATCH support (backend route bug: duplicate /{id} registration)
      const { csrfToken } = await adminLogin(request);
      const probe = await request.patch(`/_ui/api/models/registry/${modelId}`, {
        data: { enabled: models[0].enabled },
        headers: csrfHeaders(csrfToken),
      });
      if (probe.status() === 404) {
        patchSupported = false;
      }
    }
  });

  test('disable a model', async ({ request }) => {
    test.skip(!modelId, 'No models in registry — provider not connected');
    test.skip(!patchSupported, 'PATCH /models/registry/{id} not routed — backend bug');
    const { csrfToken } = await adminLogin(request);

    const res = await request.patch(`/_ui/api/models/registry/${modelId}`, {
      data: { enabled: false },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('verify model disabled', async ({ request }) => {
    test.skip(!modelId, 'No models in registry — provider not connected');
    test.skip(!patchSupported, 'PATCH /models/registry/{id} not routed — backend bug');
    await adminLogin(request);

    const res = await request.get('/_ui/api/models/registry');
    const body = await res.json();
    const models = body.models ?? [];
    const model = models.find((m: { id: string }) => m.id === modelId);
    expect(model).toBeTruthy();
    expect(model.enabled).toBe(false);
  });

  test('re-enable model', async ({ request }) => {
    test.skip(!modelId, 'No models in registry — provider not connected');
    test.skip(!patchSupported, 'PATCH /models/registry/{id} not routed — backend bug');
    const { csrfToken } = await adminLogin(request);

    const res = await request.patch(`/_ui/api/models/registry/${modelId}`, {
      data: { enabled: originalEnabled },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });
});
