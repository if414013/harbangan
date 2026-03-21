import { test, expect } from '@playwright/test';
import * as OTPAuth from 'otpauth';

// SSO config tests modify shared state — run serially
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
  const match = setCookie.match(/csrf_token=([^;]+)/);
  const csrfToken = match ? match[1] : '';
  expect(csrfToken).toBeTruthy();

  return { csrfToken };
}

function csrf(token: string): Record<string, string> {
  return { 'x-csrf-token': token };
}

// ── Cleanup: restore SSO fields to empty after all tests ────────────

test.afterAll(async ({ request }) => {
  const { csrfToken } = await adminLogin(request);
  await request.put('/_ui/api/config', {
    data: {
      google_client_id: '',
      google_client_secret: '',
      google_callback_url: '',
      auth_google_enabled: false,
    },
    headers: csrf(csrfToken),
  });
});

// ── GET /config: SSO fields present ─────────────────────────────────

test.describe('SSO Config API — GET fields', () => {
  test('GET /_ui/api/config includes google SSO fields', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/config');
    expect(res.status()).toBe(200);
    const body = await res.json();
    const config = body.config;

    expect(config).toHaveProperty('google_client_id');
    expect(config).toHaveProperty('google_client_secret');
    expect(config).toHaveProperty('google_callback_url');
    expect(config).toHaveProperty('auth_google_enabled');
    expect(config).toHaveProperty('auth_password_enabled');

    expect(typeof config.google_client_id).toBe('string');
    expect(typeof config.google_client_secret).toBe('string');
    expect(typeof config.google_callback_url).toBe('string');
  });

  test('GET /_ui/api/config/schema includes SSO field metadata', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/config/schema');
    expect(res.status()).toBe(200);
    const body = await res.json();
    const fields = body.fields;

    for (const key of ['google_client_id', 'google_client_secret', 'google_callback_url']) {
      expect(fields).toHaveProperty(key);
      expect(fields[key]).toHaveProperty('description');
      expect(fields[key]).toHaveProperty('type');
      expect(fields[key].requires_restart).toBe(false);
    }

    // google_client_secret should be typed as "password"
    expect(fields.google_client_secret.type).toBe('password');
    // Others should be "string"
    expect(fields.google_client_id.type).toBe('string');
    expect(fields.google_callback_url.type).toBe('string');

    // Auth toggles
    expect(fields).toHaveProperty('auth_google_enabled');
    expect(fields.auth_google_enabled.type).toBe('boolean');
    expect(fields).toHaveProperty('auth_password_enabled');
    expect(fields.auth_password_enabled.type).toBe('boolean');
  });
});

// ── PUT /config: SSO field persistence ──────────────────────────────

test.describe('SSO Config API — PUT persistence', () => {
  test('PUT persists google_client_id', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const testId = `test-client-id-${Date.now()}`;

    const putRes = await request.put('/_ui/api/config', {
      data: { google_client_id: testId },
      headers: csrf(csrfToken),
    });
    expect(putRes.status()).toBe(200);
    const putBody = await putRes.json();
    expect(putBody.updated).toContain('google_client_id');
    expect(putBody.hot_reloaded).toContain('google_client_id');

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.google_client_id).toBe(testId);
  });

  test('PUT persists google_callback_url', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const testUrl = 'http://localhost:9999/_ui/api/auth/google/callback';

    const putRes = await request.put('/_ui/api/config', {
      data: { google_callback_url: testUrl },
      headers: csrf(csrfToken),
    });
    expect(putRes.status()).toBe(200);

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.google_callback_url).toBe(testUrl);
  });

  test('PUT persists google_client_secret and returns masked', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const testSecret = 'my-super-secret-client-value-12345';

    const putRes = await request.put('/_ui/api/config', {
      data: { google_client_secret: testSecret },
      headers: csrf(csrfToken),
    });
    expect(putRes.status()).toBe(200);

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    // Should be masked, not the raw value
    expect(config.google_client_secret).not.toBe(testSecret);
    expect(config.google_client_secret).not.toBe('');
    // Masking format: "prefix...suffix" for strings > 8 chars, or "****" for short ones
    expect(config.google_client_secret).toMatch(/\.\.\.|^\*{4}$/);
  });

  test('PUT all three SSO fields at once', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const suffix = Date.now();
    const updates = {
      google_client_id: `batch-id-${suffix}`,
      google_client_secret: `batch-secret-${suffix}-long-enough`,
      google_callback_url: `http://localhost:9999/_ui/api/auth/google/callback`,
    };

    const putRes = await request.put('/_ui/api/config', {
      data: updates,
      headers: csrf(csrfToken),
    });
    expect(putRes.status()).toBe(200);
    const putBody = await putRes.json();
    expect(putBody.updated).toContain('google_client_id');
    expect(putBody.updated).toContain('google_client_secret');
    expect(putBody.updated).toContain('google_callback_url');

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.google_client_id).toBe(updates.google_client_id);
    expect(config.google_callback_url).toBe(updates.google_callback_url);
    // Secret should be masked
    expect(config.google_client_secret).toMatch(/\.\.\.|^\*{4}$/);
  });

  test('PUT clears google_client_id with empty string', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const putRes = await request.put('/_ui/api/config', {
      data: { google_client_id: '' },
      headers: csrf(csrfToken),
    });
    expect(putRes.status()).toBe(200);

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.google_client_id).toBe('');
  });
});

// ── Secret masking behavior ─────────────────────────────────────────

test.describe('SSO Config API — secret masking', () => {
  test('GET never exposes raw google_client_secret', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const rawSecret = `raw-secret-should-not-appear-${Date.now()}`;

    await request.put('/_ui/api/config', {
      data: { google_client_secret: rawSecret },
      headers: csrf(csrfToken),
    });

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.google_client_secret).not.toContain(rawSecret);
  });

  test('PUT with masked sentinel value is a no-op', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const originalSecret = `original-secret-${Date.now()}-long-value`;

    // Set original secret
    await request.put('/_ui/api/config', {
      data: { google_client_secret: originalSecret },
      headers: csrf(csrfToken),
    });

    // Read masked value
    const getRes1 = await request.get('/_ui/api/config');
    const maskedValue = (await getRes1.json()).config.google_client_secret;

    // PUT back the masked value — should be treated as no-op
    const { csrfToken: csrf2 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { google_client_secret: maskedValue },
      headers: csrf(csrf2),
    });

    // Read again — should still show same masked value (original unchanged)
    const getRes2 = await request.get('/_ui/api/config');
    const maskedValue2 = (await getRes2.json()).config.google_client_secret;
    expect(maskedValue2).toBe(maskedValue);
  });

  test('PUT with new value changes the masked output', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    await request.put('/_ui/api/config', {
      data: { google_client_secret: 'first-secret-value-long' },
      headers: csrf(csrfToken),
    });

    const getRes1 = await request.get('/_ui/api/config');
    const masked1 = (await getRes1.json()).config.google_client_secret;

    const { csrfToken: csrf2 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { google_client_secret: 'different-secret-value-long' },
      headers: csrf(csrf2),
    });

    const getRes2 = await request.get('/_ui/api/config');
    const masked2 = (await getRes2.json()).config.google_client_secret;

    // Masked values should differ since the underlying secrets differ
    expect(masked2).not.toBe(masked1);
  });

  test('empty google_client_secret returns empty string', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    await request.put('/_ui/api/config', {
      data: { google_client_secret: '' },
      headers: csrf(csrfToken),
    });

    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.google_client_secret).toBe('');
  });
});

// ── Validation ──────────────────────────────────────────────────────

test.describe('SSO Config API — validation', () => {
  test('rejects control characters in google_client_id', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    for (const bad of ['id\nwith\nnewlines', 'id\x00null', 'id\twith\ttabs', 'id\rwith\rcr']) {
      const res = await request.put('/_ui/api/config', {
        data: { google_client_id: bad },
        headers: csrf(csrfToken),
      });
      expect(res.status()).toBe(400);
    }
  });

  test('rejects control characters in google_callback_url', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { google_callback_url: 'http://localhost\n/callback' },
      headers: csrf(csrfToken),
    });
    expect(res.status()).toBe(400);
  });

  test('rejects control characters in google_client_secret', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { google_client_secret: 'secret\nwith\nnewlines' },
      headers: csrf(csrfToken),
    });
    expect(res.status()).toBe(400);
  });

  test('rejects string > 512 chars for google_client_id', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { google_client_id: 'a'.repeat(513) },
      headers: csrf(csrfToken),
    });
    expect(res.status()).toBe(400);
  });

  test('accepts valid 512-char string for google_client_id', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { google_client_id: 'b'.repeat(512) },
      headers: csrf(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('rejects string > 512 chars for google_client_secret', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { google_client_secret: 's'.repeat(513) },
      headers: csrf(csrfToken),
    });
    expect(res.status()).toBe(400);
  });
});

// ── Config history ──────────────────────────────────────────────────

test.describe('SSO Config API — history', () => {
  test('config history records google_client_id change', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const marker = `history-id-${Date.now()}`;

    await request.put('/_ui/api/config', {
      data: { google_client_id: marker },
      headers: csrf(csrfToken),
    });

    const historyRes = await request.get('/_ui/api/config/history?limit=20');
    expect(historyRes.status()).toBe(200);
    const historyBody = await historyRes.json();

    const entry = historyBody.history.find(
      (h: { key: string; new_value: string }) =>
        h.key === 'google_client_id' && h.new_value === marker
    );
    expect(entry).toBeTruthy();
    expect(entry.source).toBe('web_ui');
  });

  test('config history masks google_client_secret value', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const secretValue = `history-secret-${Date.now()}-long-enough`;

    await request.put('/_ui/api/config', {
      data: { google_client_secret: secretValue },
      headers: csrf(csrfToken),
    });

    const historyRes = await request.get('/_ui/api/config/history?limit=20');
    const historyBody = await historyRes.json();

    const entry = historyBody.history.find(
      (h: { key: string }) => h.key === 'google_client_secret'
    );
    expect(entry).toBeTruthy();
    // Value should be masked, not the raw secret
    expect(entry.new_value).not.toBe(secretValue);
  });
});

// ── Status endpoint integration ─────────────────────────────────────

test.describe('SSO Config API — status endpoint', () => {
  test('status reflects auth_google_enabled=false when SSO disabled', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    await request.put('/_ui/api/config', {
      data: { auth_google_enabled: false },
      headers: csrf(csrfToken),
    });

    const statusRes = await request.get('/_ui/api/status');
    expect(statusRes.status()).toBe(200);
    const status = await statusRes.json();
    expect(status.auth_google_enabled).toBe(false);
  });

  test('google_configured is false when credentials missing', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Clear all SSO fields
    await request.put('/_ui/api/config', {
      data: { google_client_id: '', google_client_secret: '', google_callback_url: '' },
      headers: csrf(csrfToken),
    });

    const statusRes = await request.get('/_ui/api/status');
    const status = await statusRes.json();
    expect(status.google_configured).toBe(false);
  });

  test('google_configured is true when all credentials set', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    await request.put('/_ui/api/config', {
      data: {
        google_client_id: `configured-id-${Date.now()}`,
        google_client_secret: 'configured-secret-long-enough',
        google_callback_url: 'http://localhost:9999/_ui/api/auth/google/callback',
      },
      headers: csrf(csrfToken),
    });

    const statusRes = await request.get('/_ui/api/status');
    const status = await statusRes.json();
    expect(status.google_configured).toBe(true);
  });

  test('auth_google_enabled requires google_configured', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Clear SSO creds but enable the toggle
    await request.put('/_ui/api/config', {
      data: {
        google_client_id: '',
        google_client_secret: '',
        google_callback_url: '',
        auth_google_enabled: true,
      },
      headers: csrf(csrfToken),
    });

    const statusRes = await request.get('/_ui/api/status');
    const status = await statusRes.json();
    // Even though toggle is enabled, status should say false because creds are missing
    expect(status.auth_google_enabled).toBe(false);
    expect(status.google_configured).toBe(false);
  });
});

// ── Access control ──────────────────────────────────────────────────

test.describe('SSO Config API — access control', () => {
  test('PUT without CSRF token returns 400 or 403', async ({ request }) => {
    await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { google_client_id: 'no-csrf-test' },
    });
    expect([400, 403]).toContain(res.status());
  });

  test('non-admin cannot PUT SSO config', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Create a regular user
    const testEmail = `sso-test-user-${Date.now()}@example.com`;
    const createRes = await request.post('/_ui/api/admin/users/create', {
      data: { email: testEmail, name: 'SSO Test User', password: 'TestPass123!', role: 'user' },
      headers: csrf(csrfToken),
    });
    expect(createRes.status()).toBe(200);

    // Login as regular user (no TOTP → direct session)
    const userLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: testEmail, password: 'TestPass123!' },
    });
    expect(userLoginRes.status()).toBe(200);
    const userCookies = userLoginRes.headers()['set-cookie'] ?? '';
    const csrfMatch = userCookies.match(/csrf_token=([^;]+)/);
    const userCsrf = csrfMatch ? csrfMatch[1] : '';

    // Try to update SSO config as non-admin
    const res = await request.put('/_ui/api/config', {
      data: { google_client_id: 'hacked-id' },
      headers: csrf(userCsrf),
    });
    expect(res.status()).toBe(403);
  });
});

// ── Admin password fallback ─────────────────────────────────────────

test.describe('SSO Config API — admin password fallback', () => {
  test('admin can login even when auth_password_enabled is false', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Disable password auth
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: false },
      headers: csrf(csrfToken),
    });

    // Admin should still be able to login with password
    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    expect(loginRes.status()).toBe(200);
    const loginBody = await loginRes.json();
    expect(loginBody.needs_2fa).toBe(true);

    // Complete 2FA
    const code = generateTOTP();
    const twoFaRes = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: loginBody.login_token, code },
    });
    expect(twoFaRes.status()).toBe(200);

    // Re-enable password auth for cleanup
    const setCookie = twoFaRes.headers()['set-cookie'] ?? '';
    const match = setCookie.match(/csrf_token=([^;]+)/);
    const newCsrf = match ? match[1] : '';
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: true },
      headers: csrf(newCsrf),
    });
  });

  test('non-admin cannot login when auth_password_enabled is false', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Create a regular user
    const testEmail = `fallback-test-${Date.now()}@example.com`;
    await request.post('/_ui/api/admin/users/create', {
      data: { email: testEmail, name: 'Fallback Test', password: 'TestPass123!', role: 'user' },
      headers: csrf(csrfToken),
    });

    // Disable password auth
    const { csrfToken: csrf2 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: false },
      headers: csrf(csrf2),
    });

    // Non-admin should be rejected
    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: testEmail, password: 'TestPass123!' },
    });
    expect(loginRes.status()).toBe(403);

    // Re-enable password auth
    const { csrfToken: csrf3 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: true },
      headers: csrf(csrf3),
    });
  });
});

// ── Auth toggle guard ───────────────────────────────────────────────

test.describe('SSO Config API — auth toggle guard', () => {
  test('status always shows auth_password_enabled field', async ({ request }) => {
    await adminLogin(request);

    const statusRes = await request.get('/_ui/api/status');
    const status = await statusRes.json();
    expect(status).toHaveProperty('auth_password_enabled');
    expect(typeof status.auth_password_enabled).toBe('boolean');
  });
});

// ── Cross-field auth toggle rejection (CR-R03) ─────────────────────

test.describe('SSO Config API — cross-field auth toggle rejection', () => {
  test('rejects disabling both auth methods simultaneously', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const res = await request.put('/_ui/api/config', {
      data: { auth_google_enabled: false, auth_password_enabled: false },
      headers: csrf(csrfToken),
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error).toMatch(/cannot disable both/i);
  });

  test('rejects disabling password auth when SSO is not fully configured', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Ensure SSO creds are empty (not fully configured)
    await request.put('/_ui/api/config', {
      data: { google_client_id: '', google_client_secret: '', google_callback_url: '' },
      headers: csrf(csrfToken),
    });

    // Try to disable password auth — should fail since SSO isn't configured
    const { csrfToken: csrf2 } = await adminLogin(request);
    const res = await request.put('/_ui/api/config', {
      data: { auth_password_enabled: false },
      headers: csrf(csrf2),
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error).toMatch(/cannot disable password auth/i);
  });

  test('rejects disabling password when SSO enabled but not configured', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Enable SSO toggle but leave creds empty
    await request.put('/_ui/api/config', {
      data: {
        google_client_id: '',
        google_client_secret: '',
        google_callback_url: '',
        auth_google_enabled: true,
      },
      headers: csrf(csrfToken),
    });

    // Try to disable password — should fail (SSO toggle on but not configured)
    const { csrfToken: csrf2 } = await adminLogin(request);
    const res = await request.put('/_ui/api/config', {
      data: { auth_password_enabled: false },
      headers: csrf(csrf2),
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error).toMatch(/cannot disable password auth/i);
  });

  test('allows disabling password when SSO is fully configured and enabled', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Set up full SSO config
    await request.put('/_ui/api/config', {
      data: {
        google_client_id: `guard-test-${Date.now()}`,
        google_client_secret: 'guard-test-secret-long-enough',
        google_callback_url: 'http://localhost:9999/_ui/api/auth/google/callback',
        auth_google_enabled: true,
      },
      headers: csrf(csrfToken),
    });

    // Now disabling password should succeed
    const { csrfToken: csrf2 } = await adminLogin(request);
    const res = await request.put('/_ui/api/config', {
      data: { auth_password_enabled: false },
      headers: csrf(csrf2),
    });
    expect(res.status()).toBe(200);

    // Restore password auth
    const { csrfToken: csrf3 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: true },
      headers: csrf(csrf3),
    });
  });

  test('rejects clearing SSO credentials when password auth is disabled', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Set up full SSO config and disable password
    await request.put('/_ui/api/config', {
      data: {
        google_client_id: `clear-test-${Date.now()}`,
        google_client_secret: 'clear-test-secret-long-enough',
        google_callback_url: 'http://localhost:9999/_ui/api/auth/google/callback',
        auth_google_enabled: true,
      },
      headers: csrf(csrfToken),
    });
    const { csrfToken: csrf2 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: false },
      headers: csrf(csrf2),
    });

    // Try to clear google_client_id — should fail
    const { csrfToken: csrf3 } = await adminLogin(request);
    const res = await request.put('/_ui/api/config', {
      data: { google_client_id: '' },
      headers: csrf(csrf3),
    });
    expect(res.status()).toBe(400);
    const body = await res.json();
    expect(body.error).toMatch(/cannot clear.*google/i);

    // Restore password auth for cleanup
    const { csrfToken: csrf4 } = await adminLogin(request);
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: true },
      headers: csrf(csrf4),
    });
  });

  test('allows clearing SSO credentials when password auth is enabled', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Ensure password is enabled, then clear SSO creds — should succeed
    await request.put('/_ui/api/config', {
      data: { auth_password_enabled: true },
      headers: csrf(csrfToken),
    });

    const { csrfToken: csrf2 } = await adminLogin(request);
    const res = await request.put('/_ui/api/config', {
      data: { google_client_id: '', google_client_secret: '', google_callback_url: '' },
      headers: csrf(csrf2),
    });
    expect(res.status()).toBe(200);
  });
});
