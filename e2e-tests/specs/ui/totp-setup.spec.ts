import { test, expect } from '@playwright/test'
import * as OTPAuth from 'otpauth'
import { Toast } from '../../helpers/selectors.js'
import { expectToastMessage } from '../../helpers/navigation.js'

// These tests run in ui-authenticated project with pre-auth session.
// All tests mock auth/me to prevent SessionGate from redirecting to /change-password
// (the admin user has must_change_password=true after re-seeding).

const fakeTotpSecret = 'JBSWY3DPEHPK3PXP' // well-known test secret
const fakeRecoveryCodes = [
  'ABCD-1234-EFGH',
  'IJKL-5678-MNOP',
  'QRST-9012-UVWX',
  'YZAB-3456-CDEF',
  'GHIJ-7890-KLMN',
  'OPQR-1234-STUV',
  'WXYZ-5678-ABCD',
  'EFGH-9012-IJKL',
]

/** Mock user response for SessionGate + TOTP setup API. */
async function setupMocks(page: import('@playwright/test').Page, userOverrides: Record<string, unknown> = {}) {
  await page.route('**/api/auth/me', async (route) => {
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
        ...userOverrides,
      }),
    })
  })

  await page.route('**/api/status', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ setup_complete: true }),
    })
  })

  await page.route('**/api/auth/2fa/setup', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        secret: fakeTotpSecret,
        qr_code_data_url: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==',
        recovery_codes: fakeRecoveryCodes,
      }),
    })
  })
}

test.describe('TOTP setup flow', () => {
  test('forced TOTP redirect when 2FA is required but not configured', async ({ page }) => {
    await setupMocks(page, { totp_enabled: false })

    // Navigate to any authenticated page — SessionGate should redirect to /setup-2fa
    await page.goto('/_ui/profile')
    await page.waitForURL(/setup-2fa/, { timeout: 10_000 })
  })

  test('TOTP setup page displays QR code', async ({ page }) => {
    await setupMocks(page)

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // QR code image should be visible
    await expect(page.locator('.totp-qr img')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('.totp-qr img')).toHaveAttribute('alt', 'TOTP QR code')

    // Manual secret key should be displayed as fallback
    await expect(page.locator('.totp-secret')).toBeVisible()
    await expect(page.locator('.totp-secret')).toHaveText(fakeTotpSecret)
  })

  test('TOTP setup shows app instructions', async ({ page }) => {
    await setupMocks(page)

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // Step indicator should show scan step
    await expect(page.locator('.step-active')).toContainText('scan')

    // Instructions text about scanning QR code with authenticator app
    await expect(page.getByText('scan this QR code with your authenticator app')).toBeVisible({ timeout: 5_000 })

    // "or enter this key manually" fallback text
    await expect(page.getByText('or enter this key manually')).toBeVisible()
  })

  test('entering valid TOTP code completes setup', async ({ page }) => {
    await setupMocks(page)

    await page.route('**/api/auth/2fa/verify', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
    })

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // Click "$ next" to move to verify step
    await page.locator('button.auth-submit').filter({ hasText: '$ next' }).click()

    // Verify step should be active
    await expect(page.locator('.step-active')).toContainText('verify')
    await expect(page.locator('input.auth-input.totp-input')).toBeVisible()

    // Generate and enter a valid TOTP code
    const totp = new OTPAuth.TOTP({
      issuer: 'KiroGateway',
      label: 'test@example.com',
      algorithm: 'SHA1',
      digits: 6,
      period: 30,
      secret: OTPAuth.Secret.fromBase32(fakeTotpSecret),
    })
    await page.locator('input.auth-input.totp-input').fill(totp.generate())
    await page.locator('button.auth-submit').filter({ hasText: /verify/ }).click()

    // Should transition to recovery codes step
    await expect(page.getByText('RECOVERY CODES')).toBeVisible({ timeout: 5_000 })
  })

  test('entering invalid TOTP code shows error', async ({ page }) => {
    await setupMocks(page)

    await page.route('**/api/auth/2fa/verify', async (route) => {
      await route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Invalid TOTP code' }),
      })
    })

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // Navigate to verify step
    await page.locator('button.auth-submit').filter({ hasText: '$ next' }).click()
    await expect(page.locator('input.auth-input.totp-input')).toBeVisible()

    // Enter invalid code
    await page.locator('input.auth-input.totp-input').fill('000000')
    await page.locator('button.auth-submit').filter({ hasText: /verify/ }).click()

    // Error should be displayed
    await expect(page.locator('.login-error')).toBeVisible({ timeout: 5_000 })
  })

  test('recovery codes displayed after successful TOTP setup', async ({ page }) => {
    await setupMocks(page)

    await page.route('**/api/auth/2fa/verify', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
    })

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // Go to verify step and submit
    await page.locator('button.auth-submit').filter({ hasText: '$ next' }).click()
    await page.locator('input.auth-input.totp-input').fill('123456')
    await page.locator('button.auth-submit').filter({ hasText: /verify/ }).click()

    // Recovery codes screen
    await expect(page.getByText('RECOVERY CODES')).toBeVisible({ timeout: 5_000 })
    await expect(page.getByText('save these codes in a secure location')).toBeVisible()

    // Each recovery code should be listed
    const codeItems = page.locator('.recovery-codes-item')
    await expect(codeItems).toHaveCount(fakeRecoveryCodes.length)

    for (const code of fakeRecoveryCodes) {
      await expect(page.locator('.recovery-codes-item').filter({ hasText: code })).toBeVisible()
    }
  })

  test('recovery codes can be copied or downloaded', async ({ page }) => {
    await setupMocks(page)

    await page.route('**/api/auth/2fa/verify', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
    })

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // Complete to recovery codes step
    await page.locator('button.auth-submit').filter({ hasText: '$ next' }).click()
    await page.locator('input.auth-input.totp-input').fill('123456')
    await page.locator('button.auth-submit').filter({ hasText: /verify/ }).click()
    await expect(page.getByText('RECOVERY CODES')).toBeVisible({ timeout: 5_000 })

    // Copy button should exist
    await expect(page.locator('button.auth-submit').filter({ hasText: '$ copy codes' })).toBeVisible()

    // Download button should exist
    await expect(page.locator('button.auth-submit.auth-submit-secondary').filter({ hasText: '$ download codes' })).toBeVisible()
  })

  test('acknowledging recovery codes completes setup flow', async ({ page }) => {
    await setupMocks(page)

    await page.route('**/api/auth/2fa/verify', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
    })

    await page.goto('/_ui/setup-2fa')
    await page.waitForLoadState('networkidle')

    // Complete to recovery codes step
    await page.locator('button.auth-submit').filter({ hasText: '$ next' }).click()
    await page.locator('input.auth-input.totp-input').fill('123456')
    await page.locator('button.auth-submit').filter({ hasText: /verify/ }).click()
    await expect(page.getByText('RECOVERY CODES')).toBeVisible({ timeout: 5_000 })

    // Click "I've saved my codes" to complete
    await page.locator('button.auth-submit').filter({ hasText: /saved my codes/ }).click()

    // Should redirect to dashboard/home
    await page.waitForURL(/\/_ui\//, { timeout: 10_000 })
  })
})
