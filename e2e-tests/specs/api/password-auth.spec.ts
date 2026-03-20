import { test, expect } from '@playwright/test';
import * as OTPAuth from 'otpauth';

// Tests modify shared state (admin password, rate limiter) — must run serially
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

/** Login admin through password + 2FA and return CSRF token + user ID. */
async function adminLogin(request: import('@playwright/test').APIRequestContext): Promise<{
  csrfToken: string;
  userId: string;
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
  const twoFaBody = await twoFaRes.json();

  const setCookie = twoFaRes.headers()['set-cookie'] ?? '';
  let csrfToken = '';
  const match = setCookie.match(/csrf_token=([^;]+)/);
  if (match) csrfToken = match[1];
  expect(csrfToken).toBeTruthy();

  return { csrfToken, userId: twoFaBody.user_id };
}

// ── Password Authentication ─────────────────────────────────────────

test.describe('Password authentication API', () => {
  test('login with valid credentials returns 2FA challenge', async ({ request }) => {
    const response = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    expect(response.status()).toBe(200);
    const body = await response.json();
    expect(body.needs_2fa).toBe(true);
    expect(body.login_token).toBeTruthy();
    expect(body.login_token).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
    );
  });

  test('login with wrong password returns 401', async ({ request }) => {
    const response = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: 'wrong-password-here' },
    });
    expect(response.status()).toBe(401);
    const body = await response.json();
    expect(body.error).toBeTruthy();
  });

  test('login with non-existent user returns 401', async ({ request }) => {
    const response = await request.post('/_ui/api/auth/login', {
      data: { email: 'nonexistent-user@example.com', password: 'anything' },
    });
    expect(response.status()).toBe(401);
  });

  test('complete 2FA with valid TOTP code', async ({ request }) => {
    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    expect(loginRes.status()).toBe(200);
    const loginBody = await loginRes.json();

    const code = generateTOTP();
    const twoFaRes = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: loginBody.login_token, code },
    });
    expect(twoFaRes.status()).toBe(200);
    const body = await twoFaRes.json();
    expect(body.ok).toBe(true);
    expect(body.user_id).toBeTruthy();

    const setCookie = twoFaRes.headers()['set-cookie'] ?? '';
    expect(setCookie).toContain('kgw_session=');
    expect(setCookie).toContain('csrf_token=');
  });

  test('complete 2FA with invalid TOTP code returns 401', async ({ request }) => {
    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    const loginBody = await loginRes.json();

    const twoFaRes = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: loginBody.login_token, code: '000000' },
    });
    expect(twoFaRes.status()).toBe(401);
  });

  test('2FA with invalid login_token returns 401', async ({ request }) => {
    const response = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: '00000000-0000-0000-0000-000000000000', code: '123456' },
    });
    expect(response.status()).toBe(401);
  });

  test('login_token is single-use', async ({ request }) => {
    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    const loginBody = await loginRes.json();
    const loginToken = loginBody.login_token;

    const code = generateTOTP();
    const firstAttempt = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: loginToken, code },
    });
    expect(firstAttempt.status()).toBe(200);

    const secondAttempt = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: loginToken, code },
    });
    expect(secondAttempt.status()).toBe(401);
  });

  test('rate limiting after repeated failed attempts', async ({ request }) => {
    // Create a dedicated user for rate limiting (avoids locking out admin)
    const { csrfToken } = await adminLogin(request);
    const rlEmail = `ratelimit-${Date.now()}@example.com`;

    const createRes = await request.post('/_ui/api/admin/users/create', {
      data: { email: rlEmail, name: 'Rate Limit Test', password: 'RateLimitPass123!', role: 'user' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(createRes.status()).toBe(200);

    // 5 failed attempts against the real user (MAX_LOGIN_ATTEMPTS = 5)
    for (let i = 0; i < 5; i++) {
      await request.post('/_ui/api/auth/login', {
        data: { email: rlEmail, password: 'wrong' },
      });
    }

    // 6th attempt should be rate limited
    const response = await request.post('/_ui/api/auth/login', {
      data: { email: rlEmail, password: 'wrong' },
    });
    expect(response.status()).toBe(429);
    const retryAfter = response.headers()['retry-after'];
    expect(retryAfter).toBeTruthy();
  });
});

// ── Admin User Management ───────────────────────────────────────────

test.describe('Admin user management API', () => {
  test('admin creates user with password', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const testEmail = `testuser-${Date.now()}@example.com`;

    const response = await request.post('/_ui/api/admin/users/create', {
      data: {
        email: testEmail,
        name: 'Test User',
        password: 'TestPassword123!',
        role: 'user',
      },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(response.status()).toBe(200);
    const body = await response.json();
    expect(body.ok).toBe(true);
    expect(body.user_id).toBeTruthy();
    expect(body.email).toBe(testEmail);
    expect(body.role).toBe('user');
  });

  test('admin resets user password', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const testEmail = `resetuser-${Date.now()}@example.com`;

    const createRes = await request.post('/_ui/api/admin/users/create', {
      data: {
        email: testEmail,
        name: 'Reset Test User',
        password: 'OldPassword123!',
        role: 'user',
      },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(createRes.status()).toBe(200);
    const { user_id } = await createRes.json();

    const { csrfToken: freshCsrf } = await adminLogin(request);

    const resetRes = await request.post(`/_ui/api/admin/users/${user_id}/reset-password`, {
      data: { new_password: 'ResetPassword123!' },
      headers: { 'x-csrf-token': freshCsrf },
    });
    expect(resetRes.status()).toBe(200);
    const body = await resetRes.json();
    expect(body.ok).toBe(true);

    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: testEmail, password: 'ResetPassword123!' },
    });
    expect(loginRes.status()).toBe(200);
    const loginBody = await loginRes.json();
    expect(loginBody.ok).toBe(true);
    expect(loginBody.must_change_password).toBe(true);
  });

  test('non-admin cannot create users', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Create regular user
    const testEmail = `regularuser-${Date.now()}@example.com`;
    await request.post('/_ui/api/admin/users/create', {
      data: { email: testEmail, name: 'Regular User', password: 'RegularPass123!', role: 'user' },
      headers: { 'x-csrf-token': csrfToken },
    });

    // Login as regular user (no TOTP → direct session with cookies)
    const userLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: testEmail, password: 'RegularPass123!' },
    });
    expect(userLoginRes.status()).toBe(200);
    const userCookies = userLoginRes.headers()['set-cookie'] ?? '';
    const csrfMatch = userCookies.match(/csrf_token=([^;]+)/);
    const userCsrf = csrfMatch ? csrfMatch[1] : '';

    // Try to create user as non-admin → 403
    const response = await request.post('/_ui/api/admin/users/create', {
      data: {
        email: `should-fail-${Date.now()}@example.com`,
        name: 'Should Fail',
        password: 'ShouldFail123!',
        role: 'user',
      },
      headers: { 'x-csrf-token': userCsrf },
    });
    expect(response.status()).toBe(403);
  });

  test('non-admin cannot reset passwords', async ({ request }) => {
    const { csrfToken, userId } = await adminLogin(request);

    // Create regular user
    const testEmail = `nonadmin-reset-${Date.now()}@example.com`;
    await request.post('/_ui/api/admin/users/create', {
      data: { email: testEmail, name: 'Non-Admin User', password: 'NonAdminPass123!', role: 'user' },
      headers: { 'x-csrf-token': csrfToken },
    });

    // Login as regular user
    const userLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: testEmail, password: 'NonAdminPass123!' },
    });
    expect(userLoginRes.status()).toBe(200);
    const userCookies = userLoginRes.headers()['set-cookie'] ?? '';
    const csrfMatch = userCookies.match(/csrf_token=([^;]+)/);
    const userCsrf = csrfMatch ? csrfMatch[1] : '';

    // Try to reset admin's password as non-admin → 403
    const response = await request.post(`/_ui/api/admin/users/${userId}/reset-password`, {
      data: { new_password: 'HackedPass123!' },
      headers: { 'x-csrf-token': userCsrf },
    });
    expect(response.status()).toBe(403);
  });
});

// ── Password Change ─────────────────────────────────────────────────

test.describe('Password change API', () => {
  test('change password with wrong current password returns 401', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    const response = await request.post('/_ui/api/auth/password/change', {
      data: { current_password: 'definitely-wrong', new_password: 'NewSecurePass123!' },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(response.status()).toBe(401);
  });

  test('change password with valid current password succeeds then revert', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);
    const newPassword = 'TempNewPassword123!';

    // Change to new password
    const changeRes = await request.post('/_ui/api/auth/password/change', {
      data: { current_password: ADMIN_PASSWORD, new_password: newPassword },
      headers: { 'x-csrf-token': csrfToken },
    });
    expect(changeRes.status()).toBe(200);
    expect((await changeRes.json()).ok).toBe(true);

    // Verify old password no longer works
    const oldLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    expect(oldLoginRes.status()).toBe(401);

    // Verify new password works (returns 2FA challenge)
    const newLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: newPassword },
    });
    expect(newLoginRes.status()).toBe(200);
    const newLoginBody = await newLoginRes.json();
    expect(newLoginBody.needs_2fa).toBe(true);

    // Complete 2FA to get new session for revert
    const code = generateTOTP();
    const twoFaRes = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: newLoginBody.login_token, code },
    });
    expect(twoFaRes.status()).toBe(200);
    const setCookie = twoFaRes.headers()['set-cookie'] ?? '';
    const match = setCookie.match(/csrf_token=([^;]+)/);
    const newCsrf = match ? match[1] : '';

    // Revert to original password
    const revertRes = await request.post('/_ui/api/auth/password/change', {
      data: { current_password: newPassword, new_password: ADMIN_PASSWORD },
      headers: { 'x-csrf-token': newCsrf },
    });
    expect(revertRes.status()).toBe(200);
  });
});
