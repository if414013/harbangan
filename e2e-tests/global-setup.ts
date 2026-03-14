import fs from 'node:fs';
import path from 'node:path';
import { adminLogin } from './helpers/auth';

export default async function globalSetup() {
  const sessionDir = path.resolve(__dirname, '.auth');
  const sessionPath = path.join(sessionDir, 'session.json');

  const hasEnvVars =
    process.env.INITIAL_ADMIN_EMAIL &&
    process.env.INITIAL_ADMIN_PASSWORD &&
    process.env.INITIAL_ADMIN_TOTP_SECRET;

  if (hasEnvVars) {
    // Automated login via password + TOTP
    const gatewayUrl = process.env.GATEWAY_URL || 'http://localhost:9999';

    console.log('Authenticating admin via password + TOTP...');
    const storageState = await adminLogin(gatewayUrl);

    if (!fs.existsSync(sessionDir)) {
      fs.mkdirSync(sessionDir, { recursive: true });
    }
    fs.writeFileSync(sessionPath, JSON.stringify(storageState, null, 2));
    console.log('✓ Session created via automated login');
    return;
  }

  // Fallback: validate existing session file
  if (!fs.existsSync(sessionPath)) {
    console.warn(
      '\n⚠  No session file and no INITIAL_ADMIN_* env vars.\n' +
        '   Set INITIAL_ADMIN_EMAIL, INITIAL_ADMIN_PASSWORD, INITIAL_ADMIN_TOTP_SECRET in .env\n' +
        '   or run "npm run test:setup" to capture a session interactively.\n' +
        '   Only the "public" project will work without it.\n'
    );
    return;
  }

  const raw = fs.readFileSync(sessionPath, 'utf-8');
  let state: { cookies?: Array<{ name: string }> };
  try {
    state = JSON.parse(raw);
  } catch {
    throw new Error('e2e-tests/.auth/session.json is not valid JSON');
  }

  const cookies = state.cookies ?? [];
  const hasSession = cookies.some(c => c.name === 'kgw_session');
  const hasCsrf = cookies.some(c => c.name === 'csrf_token');

  if (!hasSession || !hasCsrf) {
    const missing = [!hasSession && 'kgw_session', !hasCsrf && 'csrf_token']
      .filter(Boolean)
      .join(', ');
    throw new Error(
      `e2e-tests/.auth/session.json is missing required cookies: ${missing}\n` +
        'Run "npm run test:setup" to capture a fresh session.'
    );
  }

  console.log('✓ Session file validated (kgw_session + csrf_token present)');
}
