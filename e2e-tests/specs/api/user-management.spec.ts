import { test, expect } from '@playwright/test';
import { adminLogin, csrfHeaders } from '../../helpers/csrf';

// User management tests modify shared state — run serially
test.describe.configure({ mode: 'serial' });

test.describe('User Management — CRUD Lifecycle', () => {
  let csrfToken: string;
  let testUserId: string;
  const testEmail = `usermgmt-${Date.now()}@example.com`;

  test('create test user', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.post('/_ui/api/admin/users/create', {
      data: { email: testEmail, name: 'User Mgmt Test', password: 'UserMgmt123!', role: 'user' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
    expect(body.user_id).toBeTruthy();
    testUserId = body.user_id;
  });

  test('list users includes the new user', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/users');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body).toHaveProperty('users');
    expect(Array.isArray(body.users)).toBe(true);

    const found = body.users.find((u: { id: string }) => u.id === testUserId);
    expect(found).toBeTruthy();
    expect(found.email).toBe(testEmail);
    expect(found.role).toBe('user');
  });

  test('get user detail', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get(`/_ui/api/users/${testUserId}`);
    expect(res.status()).toBe(200);
    const body = await res.json();
    // Detail endpoint wraps user in { user: {...}, api_keys, kiro_status }
    expect(body.user.id).toBe(testUserId);
    expect(body.user.email).toBe(testEmail);
    expect(body.user.role).toBe('user');
  });

  test('change role to admin', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.put(`/_ui/api/users/${testUserId}/role`, {
      data: { role: 'admin' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);

    // Verify via GET
    const detailRes = await request.get(`/_ui/api/users/${testUserId}`);
    const detail = await detailRes.json();
    expect(detail.user.role).toBe('admin');
  });

  test('change role back to user', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.put(`/_ui/api/users/${testUserId}/role`, {
      data: { role: 'user' },
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('delete user', async ({ request }) => {
    ({ csrfToken } = await adminLogin(request));

    const res = await request.delete(`/_ui/api/users/${testUserId}`, {
      headers: csrfHeaders(csrfToken),
    });
    expect(res.status()).toBe(200);
  });

  test('deleted user no longer in list', async ({ request }) => {
    await adminLogin(request);

    const res = await request.get('/_ui/api/users');
    expect(res.status()).toBe(200);
    const body = await res.json();

    const found = body.users.find((u: { id: string }) => u.id === testUserId);
    expect(found).toBeUndefined();
  });
});

test.describe('User Management — RBAC', () => {
  test('non-admin cannot list users', async ({ request }) => {
    const { csrfToken } = await adminLogin(request);

    // Create a regular user
    const userEmail = `usermgmt-rbac-${Date.now()}@example.com`;
    await request.post('/_ui/api/admin/users/create', {
      data: { email: userEmail, name: 'RBAC Test', password: 'RbacTest123!', role: 'user' },
      headers: csrfHeaders(csrfToken),
    });

    // Login as regular user (no 2FA required for new password users)
    const userLoginRes = await request.post('/_ui/api/auth/login', {
      data: { email: userEmail, password: 'RbacTest123!' },
    });
    expect(userLoginRes.status()).toBe(200);

    // Non-admin should get 403 on admin routes
    const listRes = await request.get('/_ui/api/users');
    expect(listRes.status()).toBe(403);
  });
});
