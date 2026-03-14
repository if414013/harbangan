import { test, expect } from '@playwright/test';
import * as OTPAuth from 'otpauth';

// Config API tests modify shared state — run serially to avoid races
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

// ── Config GET: Provider OAuth fields ────────────────────────────────

test.describe('Config API — provider OAuth fields', () => {
  test('GET /_ui/api/config includes provider OAuth client IDs', async ({ request }) => {
    await adminLogin(request);

    const response = await request.get('/_ui/api/config');
    expect(response.status()).toBe(200);
    const body = await response.json();

    expect(body).toHaveProperty('config');
    const config = body.config;

    // All three provider OAuth client ID fields must be present
    expect(config).toHaveProperty('qwen_oauth_client_id');
    expect(config).toHaveProperty('anthropic_oauth_client_id');
    expect(config).toHaveProperty('openai_oauth_client_id');

    // Values should be strings (possibly empty default)
    expect(typeof config.qwen_oauth_client_id).toBe('string');
    expect(typeof config.anthropic_oauth_client_id).toBe('string');
    expect(typeof config.openai_oauth_client_id).toBe('string');
  });

  test('GET /_ui/api/config/schema includes provider OAuth field metadata', async ({ request }) => {
    await adminLogin(request);

    const response = await request.get('/_ui/api/config/schema');
    expect(response.status()).toBe(200);
    const body = await response.json();

    expect(body).toHaveProperty('fields');
    const fields = body.fields;

    // Each provider OAuth field should have schema metadata
    for (const key of ['qwen_oauth_client_id', 'anthropic_oauth_client_id', 'openai_oauth_client_id']) {
      expect(fields).toHaveProperty(key);
      expect(fields[key]).toHaveProperty('description');
      expect(fields[key]).toHaveProperty('type');
      expect(fields[key].type).toBe('string');
      // All three are HotReload (requires_restart = false)
      expect(fields[key].requires_restart).toBe(false);
    }
  });
});

// ── Config PUT: Provider OAuth persistence ───────────────────────────

test.describe('Config API — provider OAuth updates', () => {
  test('PUT /_ui/api/config accepts and persists a provider OAuth client ID', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const testClientId = `test-e2e-client-${Date.now()}`;

    // Update qwen_oauth_client_id
    const putRes = await request.put('/_ui/api/config', {
      data: { qwen_oauth_client_id: testClientId },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(putRes.status()).toBe(200);
    const putBody = await putRes.json();
    expect(putBody.updated).toContain('qwen_oauth_client_id');
    expect(putBody.hot_reloaded).toContain('qwen_oauth_client_id');
    expect(putBody.requires_restart).not.toContain('qwen_oauth_client_id');

    // Verify persisted via GET
    const getRes = await request.get('/_ui/api/config');
    expect(getRes.status()).toBe(200);
    const getBody = await getRes.json();
    expect(getBody.config.qwen_oauth_client_id).toBe(testClientId);
  });

  test('PUT /_ui/api/config can update all three provider OAuth IDs at once', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const suffix = Date.now();
    const updates = {
      qwen_oauth_client_id: `qwen-e2e-${suffix}`,
      anthropic_oauth_client_id: `anthropic-e2e-${suffix}`,
      openai_oauth_client_id: `openai-e2e-${suffix}`,
    };

    const putRes = await request.put('/_ui/api/config', {
      data: updates,
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(putRes.status()).toBe(200);
    const putBody = await putRes.json();

    // All three should be hot-reloaded
    for (const key of Object.keys(updates)) {
      expect(putBody.updated).toContain(key);
      expect(putBody.hot_reloaded).toContain(key);
    }

    // Verify via GET
    const getRes = await request.get('/_ui/api/config');
    const config = (await getRes.json()).config;
    expect(config.qwen_oauth_client_id).toBe(updates.qwen_oauth_client_id);
    expect(config.anthropic_oauth_client_id).toBe(updates.anthropic_oauth_client_id);
    expect(config.openai_oauth_client_id).toBe(updates.openai_oauth_client_id);
  });
});

// ── Config PUT: Validation ──────────────────────────────────────────

test.describe('Config API — provider OAuth validation', () => {
  test('rejects control characters in OAuth client IDs', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Newlines
    const res1 = await request.put('/_ui/api/config', {
      data: { qwen_oauth_client_id: 'id\nwith\nnewlines' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res1.status()).toBe(400);

    // Null bytes
    const res2 = await request.put('/_ui/api/config', {
      data: { anthropic_oauth_client_id: 'id\x00null' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res2.status()).toBe(400);

    // Tabs
    const res3 = await request.put('/_ui/api/config', {
      data: { openai_oauth_client_id: 'id\twith\ttabs' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res3.status()).toBe(400);

    // Carriage return
    const res4 = await request.put('/_ui/api/config', {
      data: { qwen_oauth_client_id: 'id\rwith\rcr' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(res4.status()).toBe(400);
  });

  test('rejects empty string for OAuth client IDs', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const response = await request.put('/_ui/api/config', {
      data: { qwen_oauth_client_id: '' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(response.status()).toBe(400);
  });

  test('rejects string exceeding 256 characters', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const tooLong = 'a'.repeat(257);
    const response = await request.put('/_ui/api/config', {
      data: { anthropic_oauth_client_id: tooLong },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(response.status()).toBe(400);
  });

  test('accepts valid 256-character string', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const maxLen = 'b'.repeat(256);
    const response = await request.put('/_ui/api/config', {
      data: { openai_oauth_client_id: maxLen },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(response.status()).toBe(200);
  });
});

// ── Config History ──────────────────────────────────────────────────

test.describe('Config API — provider OAuth history', () => {
  test('config history records provider OAuth changes', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Make a change with a unique value to find in history
    const marker = `history-e2e-${Date.now()}`;
    const putRes = await request.put('/_ui/api/config', {
      data: { anthropic_oauth_client_id: marker },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(putRes.status()).toBe(200);

    // Fetch history and find our change
    const historyRes = await request.get('/_ui/api/config/history?limit=20');
    expect(historyRes.status()).toBe(200);
    const historyBody = await historyRes.json();
    expect(historyBody).toHaveProperty('history');
    expect(Array.isArray(historyBody.history)).toBe(true);

    const entry = historyBody.history.find(
      (h: { key: string; new_value: string }) =>
        h.key === 'anthropic_oauth_client_id' && h.new_value === marker
    );
    expect(entry).toBeTruthy();
    expect(entry.changed_at).toBeTruthy();
    expect(entry.source).toBe('web_ui');
  });
});
