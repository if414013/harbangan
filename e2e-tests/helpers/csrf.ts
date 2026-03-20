import { expect } from '@playwright/test';
import type { APIRequestContext } from '@playwright/test';
import * as OTPAuth from 'otpauth';

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

/**
 * Login admin via password + 2FA using Playwright's APIRequestContext
 * and return the CSRF token for mutating requests.
 *
 * The APIRequestContext auto-manages cookies (kgw_session, csrf_token)
 * so subsequent requests on the same context are authenticated.
 * The returned csrfToken must be sent as `X-CSRF-Token` header on
 * POST/PUT/DELETE/PATCH requests to `/_ui/api/*`.
 */
export async function adminLogin(request: APIRequestContext): Promise<{
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

/**
 * Convenience: returns headers object for mutating fetch requests.
 * Usage: `request.post(url, { headers: csrfHeaders(token), data: ... })`
 */
export function csrfHeaders(csrfToken: string): Record<string, string> {
  return { 'x-csrf-token': csrfToken };
}
