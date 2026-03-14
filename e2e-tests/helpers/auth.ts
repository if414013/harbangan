import * as OTPAuth from 'otpauth';

interface Cookie {
  name: string;
  value: string;
  domain: string;
  path: string;
  expires: number;
  httpOnly: boolean;
  secure: boolean;
  sameSite: 'Strict' | 'Lax' | 'None';
}

export interface StorageState {
  cookies: Cookie[];
  origins: Array<{ origin: string; localStorage: Array<{ name: string; value: string }> }>;
}

/**
 * Perform admin login via password + TOTP and return a Playwright StorageState.
 */
export async function adminLogin(baseUrl: string): Promise<StorageState> {
  const email = process.env.INITIAL_ADMIN_EMAIL;
  const password = process.env.INITIAL_ADMIN_PASSWORD;
  const totpSecret = process.env.INITIAL_ADMIN_TOTP_SECRET;

  if (!email || !password || !totpSecret) {
    throw new Error(
      'Missing env vars: INITIAL_ADMIN_EMAIL, INITIAL_ADMIN_PASSWORD, INITIAL_ADMIN_TOTP_SECRET'
    );
  }

  // Step 1: POST /auth/login — get login_token for 2FA
  const loginRes = await fetch(`${baseUrl}/_ui/api/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password }),
  });

  if (!loginRes.ok && loginRes.status !== 200) {
    const text = await loginRes.text();
    throw new Error(`Login failed (${loginRes.status}): ${text}`);
  }

  const loginBody = await loginRes.json();

  if (!loginBody.needs_2fa || !loginBody.login_token) {
    // No 2FA — direct session (shouldn't happen with TOTP pre-configured)
    const cookies = parseCookies(loginRes.headers, baseUrl);
    return { cookies, origins: [] };
  }

  // Step 2: Generate TOTP code
  const totp = new OTPAuth.TOTP({
    issuer: 'KiroGateway',
    label: email,
    algorithm: 'SHA1',
    digits: 6,
    period: 30,
    secret: OTPAuth.Secret.fromBase32(totpSecret),
  });
  const code = totp.generate();

  // Step 3: POST /auth/login/2fa — complete login
  const twoFaRes = await fetch(`${baseUrl}/_ui/api/auth/login/2fa`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ login_token: loginBody.login_token, code }),
  });

  if (!twoFaRes.ok) {
    const text = await twoFaRes.text();
    throw new Error(`2FA verification failed (${twoFaRes.status}): ${text}`);
  }

  const cookies = parseCookies(twoFaRes.headers, baseUrl);

  const hasSession = cookies.some(c => c.name === 'kgw_session');
  const hasCsrf = cookies.some(c => c.name === 'csrf_token');
  if (!hasSession || !hasCsrf) {
    throw new Error('Login succeeded but missing session cookies in response');
  }

  return { cookies, origins: [] };
}

/**
 * Parse Set-Cookie headers into Playwright cookie format.
 */
function parseCookies(headers: Headers, baseUrl: string): Cookie[] {
  const url = new URL(baseUrl);
  const domain = url.hostname;
  const cookies: Cookie[] = [];

  // getSetCookie() returns individual Set-Cookie header values
  const setCookieHeaders = headers.getSetCookie?.() ?? [];

  for (const header of setCookieHeaders) {
    const parts = header.split(';').map(s => s.trim());
    const [nameValue] = parts;
    if (!nameValue) continue;

    const eqIdx = nameValue.indexOf('=');
    if (eqIdx < 0) continue;

    const name = nameValue.substring(0, eqIdx);
    const value = nameValue.substring(eqIdx + 1);

    const cookie: Cookie = {
      name,
      value,
      domain,
      path: '/_ui',
      expires: Math.floor(Date.now() / 1000) + 86400,
      httpOnly: false,
      secure: false,
      sameSite: 'Strict',
    };

    for (const part of parts.slice(1)) {
      const lower = part.toLowerCase();
      if (lower === 'httponly') cookie.httpOnly = true;
      else if (lower === 'secure') cookie.secure = true;
      else if (lower.startsWith('path=')) cookie.path = part.split('=')[1];
      else if (lower.startsWith('samesite=')) {
        const val = part.split('=')[1];
        if (val === 'Lax' || val === 'Strict' || val === 'None') cookie.sameSite = val;
      } else if (lower.startsWith('max-age=')) {
        cookie.expires = Math.floor(Date.now() / 1000) + parseInt(part.split('=')[1], 10);
      }
    }

    cookies.push(cookie);
  }

  return cookies;
}
