import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

// Guardrails tests modify shared state — run serially
test.describe.configure({ mode: 'serial' });

test.describe('Guardrails — Profile CRUD', () => {
  let csrfToken: string;
  let profileId: string;

  test('create profile', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/guardrails/profiles', {
      data: {
        name: `e2e-profile-${Date.now()}`,
        guardrail_id: 'e2e-test-guardrail-id',
        access_key: 'fake-access-key-for-e2e-test',
        secret_key: 'fake-secret-key-for-e2e-test',
        enabled: true,
      },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.id).toBeTruthy();
    profileId = body.id;
  });

  test('list profiles includes the new one', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/guardrails/profiles');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('profiles');
    expect(Array.isArray(body.profiles)).toBe(true);

    const found = body.profiles.find((p: { id: string }) => p.id === profileId);
    expect(found).toBeTruthy();
  });

  test('get profile by ID', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get(`/_ui/api/guardrails/profiles/${profileId}`);
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.id).toBe(profileId);
    expect(body.guardrail_id).toBe('e2e-test-guardrail-id');
  });

  test('update profile', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.put(`/_ui/api/guardrails/profiles/${profileId}`, {
      data: {
        name: `e2e-profile-updated-${Date.now()}`,
        guardrail_id: 'e2e-test-guardrail-id-updated',
        access_key: 'fake-access-key-for-e2e-test',
      },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('delete profile', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.delete(`/_ui/api/guardrails/profiles/${profileId}`, {
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('deleted profile returns 404', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get(`/_ui/api/guardrails/profiles/${profileId}`);
    expect(res.status()).toBe(404);
  });
});

test.describe('Guardrails — Rule CRUD', () => {
  let csrfToken: string;
  let profileId: string;
  let ruleId: string;

  test.beforeAll(async ({ request }) => {
    // Create a profile to attach rules to
    ({ csrfToken } = await adminLogin(request));
    const res = await request.post('/_ui/api/guardrails/profiles', {
      data: {
        name: `e2e-rule-profile-${Date.now()}`,
        guardrail_id: 'e2e-rule-test-guardrail',
        access_key: 'fake-access-key-for-e2e-test',
        secret_key: 'fake-secret-key-for-e2e-test',
        enabled: true,
      },
      headers: csrfHeaders(csrfToken),
    });
    const body = await res.json();
    profileId = body.id;
  });

  test('create rule', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/guardrails/rules', {
      data: {
        name: `e2e-rule-${Date.now()}`,
        description: 'E2E test rule',
        cel_expression: 'content.contains("test")',
        apply_to: 'input',
        profile_ids: [profileId],
      },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.id).toBeTruthy();
    ruleId = body.id;
  });

  test('list rules includes the new one', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/guardrails/rules');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('rules');
    expect(Array.isArray(body.rules)).toBe(true);

    const found = body.rules.find((r: { id: string }) => r.id === ruleId);
    expect(found).toBeTruthy();
  });

  test('get rule by ID', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get(`/_ui/api/guardrails/rules/${ruleId}`);
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.id).toBe(ruleId);
  });

  test('update rule', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.put(`/_ui/api/guardrails/rules/${ruleId}`, {
      data: {
        name: `e2e-rule-updated-${Date.now()}`,
        description: 'Updated rule description',
      },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('delete rule', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.delete(`/_ui/api/guardrails/rules/${ruleId}`, {
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test.afterAll(async ({ request }) => {
    // Cleanup: delete the profile
    ({ csrfToken } = await adminLogin(request));
    await request.delete(`/_ui/api/guardrails/profiles/${profileId}`, {
      headers: csrfHeaders(csrfToken),
    });
  });
});

test.describe('Guardrails — CEL Validation', () => {
  test('valid CEL expression returns valid: true', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/guardrails/cel/validate', {
      data: { expression: 'content.contains("hello")' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.valid).toBe(true);
  });

  test('invalid CEL expression returns valid: false', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.post('/_ui/api/guardrails/cel/validate', {
      data: { expression: 'invalid %%% syntax' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.valid).toBe(false);
    expect(body.error).toBeTruthy();
  });
});

test.describe('Guardrails — RBAC', () => {
  test('non-admin cannot access guardrails endpoints', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Create a regular user
    const userEmail = `guardrails-rbac-${Date.now()}@example.com`;
    await request.post('/_ui/api/admin/users/create', {
      data: { email: userEmail, name: 'Guardrails RBAC', password: 'GrRbac123!', role: 'user' },
      headers: csrfHeaders(csrfToken),
    });

    // Login as regular user (no 2FA required for new password users)
    const userLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: userEmail, password: 'GrRbac123!' },
    });
    expect(userLoginRes.status()).toBe(200);

    // GET profiles → 403
    const listRes = await request.get('/_ui/api/guardrails/profiles');
    expect(listRes.status()).toBe(403);
  });
});
