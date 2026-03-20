import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

test.describe('CORS', () => {
  test('response includes CORS headers', async ({ request }) => {
    const res = await request.fetch('/v1/models', {
      headers: {
        'Origin': 'http://localhost:5173',
      },
    });
    // The endpoint may 401 without auth, but CORS headers should still be present
    const corsHeader = res.headers()['access-control-allow-origin'];
    expect(corsHeader).toBeTruthy();
  });
});

test.describe('Cookie Security', () => {
  test('login sets secure cookie attributes', async ({ request }) => {
    const ADMIN_EMAIL = process.env.INITIAL_ADMIN_EMAIL!;
    const ADMIN_PASSWORD = process.env.INITIAL_ADMIN_PASSWORD!;
    const TOTP_SECRET = process.env.INITIAL_ADMIN_TOTP_SECRET!;

    // Login step 1
    const loginRes = await request.post('/_ui/api/auth/login', {
      data: { email: ADMIN_EMAIL, password: ADMIN_PASSWORD },
    });
    const loginBody = await loginRes.json();

    // Login step 2 (2FA)
    const OTPAuth = await import('otpauth');
    const totp = new OTPAuth.TOTP({
      issuer: 'KiroGateway',
      label: ADMIN_EMAIL,
      algorithm: 'SHA1',
      digits: 6,
      period: 30,
      secret: OTPAuth.Secret.fromBase32(TOTP_SECRET),
    });
    const twoFaRes = await request.post('/_ui/api/auth/login/2fa', {
      data: { login_token: loginBody.login_token, code: totp.generate() },
    });
    expect(twoFaRes.status()).toBe(200);

    const setCookie = twoFaRes.headers()['set-cookie'] ?? '';
    // Session cookie should have HttpOnly
    expect(setCookie).toContain('HttpOnly');
    // Path should be scoped to /_ui
    expect(setCookie).toContain('Path=/_ui');
    // SameSite should be set
    expect(setCookie).toMatch(/SameSite=(Strict|Lax)/);
  });
});

test.describe('CSRF Protection', () => {
  test('POST to mutating endpoint without CSRF token returns 403', async ({ request }) => {
    // Login first to get a valid session
    await adminLogin(request);

    // POST without X-CSRF-Token header — should be rejected
    const res = await request.post('/_ui/api/auth/logout');
    expect([400, 403]).toContain(res.status());
  });

  test('POST with wrong CSRF token returns 403', async ({ request }) => {
    await adminLogin(request);

    const res = await request.post('/_ui/api/auth/logout', {
      headers: { 'x-csrf-token': 'wrong-token-value' },
    });
    expect([400, 403]).toContain(res.status());
  });
});
