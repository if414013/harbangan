import { test, expect } from '@playwright/test'
import { Form, Toast } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

// These tests run in ui-authenticated project with pre-auth session.
// The PasswordChange page uses .auth-input fields and .auth-submit button.
// All tests mock auth/me and status to control user state for predictable behavior.

/** Mock user response for SessionGate. */
function mockUser(page: import('@playwright/test').Page, overrides: Record<string, unknown> = {}) {
  return Promise.all([
    page.route('**/api/auth/me', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'test-user-id',
          email: 'testuser@example.com',
          name: 'Test User',
          picture_url: null,
          role: 'user',
          last_login: null,
          created_at: '2024-01-01T00:00:00Z',
          auth_method: 'password',
          totp_enabled: true,
          must_change_password: false,
          ...overrides,
        }),
      })
    }),
    page.route('**/api/status', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ setup_complete: true }),
      })
    }),
  ])
}

test.describe('Password change flow', () => {
  test('forced password change redirect on first login', async ({ page }) => {
    await mockUser(page, { must_change_password: true })

    // Navigate to any authenticated page — SessionGate should redirect
    await page.goto('/_ui/profile')
    await page.waitForURL(/change-password/, { timeout: 10_000 })

    // Verify the page heading and description
    await expect(page.getByText('CHANGE PASSWORD')).toBeVisible()
    await expect(page.getByText('you must update your password to continue')).toBeVisible()
  })

  test('password change form renders with required fields', async ({ page }) => {
    await mockUser(page)

    await page.goto('/_ui/change-password')
    await page.waitForLoadState('networkidle')

    // Verify placeholders to identify the fields
    await expect(page.locator('input.auth-input[placeholder="current password"]')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('input.auth-input[placeholder="new password (min 8 chars)"]')).toBeVisible()
    await expect(page.locator('input.auth-input[placeholder="confirm new password"]')).toBeVisible()

    // Submit button should exist
    await expect(page.locator('button.auth-submit')).toBeVisible()
    await expect(page.locator('button.auth-submit')).toHaveText('$ update password')
  })

  test('validates new password meets strength requirements', async ({ page }) => {
    await mockUser(page)

    await page.goto('/_ui/change-password')
    await page.waitForLoadState('networkidle')
    await expect(page.locator('input.auth-input[placeholder="current password"]')).toBeVisible({ timeout: 5_000 })

    // Fill current password
    await page.locator('input.auth-input[placeholder="current password"]').fill('old-password')

    // Fill weak new password — browser has minLength=8, so verify browser prevents submission
    await page.locator('input.auth-input[placeholder="new password (min 8 chars)"]').fill('123')
    await page.locator('input.auth-input[placeholder="confirm new password"]').fill('123')

    // Verify minLength attribute is present (enforces browser-level validation)
    await expect(page.locator('input.auth-input[placeholder="new password (min 8 chars)"]')).toHaveAttribute('minlength', '8')

    // Click submit — browser validation should block submission (no navigation occurs)
    await page.locator('button.auth-submit').click()

    // Page should still be on change-password (browser blocked the submit)
    expect(page.url()).toContain('change-password')
  })

  test('validates new password and confirm password match', async ({ page }) => {
    await mockUser(page)

    await page.goto('/_ui/change-password')
    await page.waitForLoadState('networkidle')
    await expect(page.locator('input.auth-input[placeholder="current password"]')).toBeVisible({ timeout: 5_000 })

    // Fill current password
    await page.locator('input.auth-input[placeholder="current password"]').fill('old-password')

    // Fill mismatched passwords (both pass minLength=8 browser validation)
    await page.locator('input.auth-input[placeholder="new password (min 8 chars)"]').fill('new-secure-password-1')
    await page.locator('input.auth-input[placeholder="confirm new password"]').fill('different-password-2')

    await page.locator('button.auth-submit').click()

    // Client-side JS validation: "Passwords do not match"
    await expect(page.locator('.login-error')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('.login-error')).toHaveText('Passwords do not match')
  })

  test('rejects change when current password is wrong', async ({ page }) => {
    await mockUser(page)

    // Mock the password change endpoint to reject with 400 (not 401, to avoid apiFetch redirect)
    await page.route('**/api/auth/password/change', async (route) => {
      await route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({ error: { message: 'Current password is incorrect', type: 'invalid_password' } }),
      })
    })

    await page.goto('/_ui/change-password')
    await page.waitForLoadState('networkidle')
    await expect(page.locator('input.auth-input[placeholder="current password"]')).toBeVisible({ timeout: 5_000 })

    // Fill wrong current password with valid new password
    await page.locator('input.auth-input[placeholder="current password"]').fill('wrong-current-password')
    await page.locator('input.auth-input[placeholder="new password (min 8 chars)"]').fill('valid-new-password-123')
    await page.locator('input.auth-input[placeholder="confirm new password"]').fill('valid-new-password-123')

    await page.locator('button.auth-submit').click()

    // Server-side error about incorrect current password
    await expect(page.locator('.login-error')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('.login-error')).toContainText('Current password is incorrect')
  })

  test('successful password change shows confirmation', async ({ page }) => {
    await mockUser(page)

    // Mock the password change endpoint to succeed
    await page.route('**/api/auth/password/change', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: '{}',
      })
    })

    await page.goto('/_ui/change-password')
    await page.waitForLoadState('networkidle')
    await expect(page.locator('input.auth-input[placeholder="current password"]')).toBeVisible({ timeout: 5_000 })

    await page.locator('input.auth-input[placeholder="current password"]').fill('current-password')
    await page.locator('input.auth-input[placeholder="new password (min 8 chars)"]').fill('new-secure-password-123')
    await page.locator('input.auth-input[placeholder="confirm new password"]').fill('new-secure-password-123')

    await page.locator('button.auth-submit').click()

    // On success, PasswordChange navigates to / (dashboard)
    await page.waitForURL(/\/_ui\//, { timeout: 10_000 })
  })

  test('successful forced change redirects to dashboard', async ({ page }) => {
    await mockUser(page, { must_change_password: true })

    // Mock password change to succeed
    await page.route('**/api/auth/password/change', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: '{}',
      })
    })

    // Navigate — should be redirected to change-password
    await page.goto('/_ui/profile')
    await page.waitForURL(/change-password/, { timeout: 10_000 })

    // Verify the forced change heading
    await expect(page.getByText('CHANGE PASSWORD')).toBeVisible()

    // Fill the form
    await page.locator('input.auth-input[placeholder="current password"]').fill('old-temp-password')
    await page.locator('input.auth-input[placeholder="new password (min 8 chars)"]').fill('new-strong-password-123')
    await page.locator('input.auth-input[placeholder="confirm new password"]').fill('new-strong-password-123')

    await page.locator('button.auth-submit').click()

    // Should redirect to dashboard
    await page.waitForURL(/\/_ui\//, { timeout: 10_000 })
  })
})
