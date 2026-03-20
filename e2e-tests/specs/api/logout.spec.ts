import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

test.describe('Logout API', () => {
  test('login, verify session, logout, verify invalidated', async ({ request }) => {
    // Login and verify session is active
    const { csrfToken } = await adminLogin(request);
    const meRes = await request.get('/_ui/api/auth/me');
    expect(meRes.status()).toBe(200);
    const body = await meRes.json();
    expect(body.email).toBeTruthy();

    // Logout should succeed
    const logoutRes = await request.post('/_ui/api/auth/logout', {
      headers: csrfHeaders(csrfToken),
    });
    expect(logoutRes.status()).toBe(200);

    // Session should be invalidated — auth/me returns 401
    const meAfter = await request.get('/_ui/api/auth/me');
    expect(meAfter.status()).toBe(401);
  });
});
