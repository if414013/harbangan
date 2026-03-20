import { test, expect } from '@playwright/test';
import { adminLogin } from '../../helpers/csrf';

test.describe('System & Monitoring Endpoints', () => {
  test('GET /system returns system info shape', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/system');
    expect(res.status()).toBe(200);
    const body = await res.json();

    // System info should have key fields
    expect(body).toHaveProperty('cpu_usage');
    expect(body).toHaveProperty('memory_bytes');
    expect(body).toHaveProperty('uptime_seconds');
  });

  test('GET /auth/me returns current user shape', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/auth/me');
    expect(res.status()).toBe(200);
    const body = await res.json();

    expect(body).toHaveProperty('email');
    expect(body).toHaveProperty('role');
    expect(body.role).toBe('admin');
    expect(body).toHaveProperty('auth_method');
  });
});
