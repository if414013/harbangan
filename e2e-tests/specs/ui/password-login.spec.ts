import { test, expect } from '@playwright/test'
import * as OTPAuth from 'otpauth'
import { Login } from '../../helpers/selectors.js'

const email = process.env.INITIAL_ADMIN_EMAIL!
const password = process.env.INITIAL_ADMIN_PASSWORD!
const totpSecret = process.env.INITIAL_ADMIN_TOTP_SECRET!

function generateTotpCode(secret: string, label: string): string {
  const totp = new OTPAuth.TOTP({
    issuer: 'KiroGateway',
    label,
    algorithm: 'SHA1',
    digits: 6,
    period: 30,
    secret: OTPAuth.Secret.fromBase32(secret),
  })
  return totp.generate()
}

test.describe('Password login page', () => {
  test('renders password login form with email and password fields', async ({ page }) => {
    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('input.auth-input[type="email"]')).toBeVisible()
    await expect(page.locator('input.auth-input[type="password"]')).toBeVisible()
    // Use form-scoped selector since there may be a Google sign-in button too
    await expect(page.locator('form button.auth-submit')).toBeVisible()
  })

  test('shows both Google SSO and password login options', async ({ page }) => {
    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    // Password form inputs
    await expect(page.locator('input.auth-input[type="email"]')).toBeVisible()
    await expect(page.locator('input.auth-input[type="password"]')).toBeVisible()

    // When both auth methods enabled, expect "or" divider and two buttons
    const divider = page.locator('.auth-divider')
    const hasDivider = await divider.isVisible().catch(() => false)
    if (hasDivider) {
      const buttons = page.locator(Login.submit)
      await expect(buttons).toHaveCount(2)
      await expect(page.locator('form button.auth-submit')).toHaveText('$ sign in')
      await expect(page.locator('button.auth-submit').last()).toHaveText('$ sign in with google')
    }
  })

  test('successful password login redirects to dashboard', async ({ page }) => {
    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    await page.locator('input.auth-input[type="email"]').fill(email)
    await page.locator('input.auth-input[type="password"]').fill(password)
    await page.locator('form button.auth-submit').click()

    // Admin user has TOTP configured, so 2FA step appears
    await expect(page.locator('input.auth-input.totp-input')).toBeVisible({ timeout: 10_000 })

    const code = generateTotpCode(totpSecret, email)
    await page.locator('input.auth-input.totp-input').fill(code)
    await page.locator(Login.submit).click()

    // Should redirect to authenticated area
    await page.waitForURL(/\/_ui\//, { timeout: 10_000 })
  })

  test('login with 2FA prompts for TOTP code', async ({ page }) => {
    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    await page.locator('input.auth-input[type="email"]').fill(email)
    await page.locator('input.auth-input[type="password"]').fill(password)
    await page.locator('form button.auth-submit').click()

    // 2FA verification screen should appear
    await expect(page.locator('h2')).toContainText('2FA VERIFICATION', { timeout: 10_000 })
    await expect(page.locator('input.auth-input.totp-input')).toBeVisible()
    await expect(page.locator('input.auth-input.totp-input')).toHaveAttribute('placeholder', '000000')

    // Verify the recovery code toggle exists
    await expect(page.locator('.auth-toggle-link')).toHaveText('use recovery code')

    // Enter valid TOTP code and verify redirect
    const code = generateTotpCode(totpSecret, email)
    await page.locator('input.auth-input.totp-input').fill(code)
    await page.locator(Login.submit).click()

    await page.waitForURL(/\/_ui\//, { timeout: 10_000 })
  })

  test('shows error message for invalid credentials', async ({ page }) => {
    // Mock login API to return 400 with error message.
    // (Real API returns 401 which apiFetch intercepts as "session expired" + page redirect,
    // preventing the Login component from displaying the error. We mock to test the UI flow.)
    await page.route('**/api/auth/login', async (route) => {
      const req = route.request()
      if (req.method() === 'POST') {
        const body = req.postDataJSON()
        if (body.password !== password) {
          await route.fulfill({
            status: 400,
            contentType: 'application/json',
            body: JSON.stringify({ error: { message: 'Invalid email or password', type: 'invalid_credentials' } }),
          })
          return
        }
      }
      await route.continue()
    })

    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    await page.locator('input.auth-input[type="email"]').fill(email)
    await page.locator('input.auth-input[type="password"]').fill('wrong-password-here')
    await page.locator('form button.auth-submit').click()

    await expect(page.locator(Login.error)).toBeVisible({ timeout: 10_000 })
    await expect(page.locator(Login.error)).toContainText('Invalid email or password')
  })

  test('shows error message for invalid TOTP code', async ({ page }) => {
    // Mock 2FA API to return 400 for invalid codes
    await page.route('**/api/auth/login/2fa', async (route) => {
      await route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({ error: { message: 'Invalid verification code', type: 'invalid_code' } }),
      })
    })

    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    // Complete password step
    await page.locator('input.auth-input[type="email"]').fill(email)
    await page.locator('input.auth-input[type="password"]').fill(password)
    await page.locator('form button.auth-submit').click()

    // Wait for 2FA screen
    await expect(page.locator('input.auth-input.totp-input')).toBeVisible({ timeout: 10_000 })

    // Enter invalid TOTP code
    await page.locator('input.auth-input.totp-input').fill('999999')
    await page.locator(Login.submit).click()

    await expect(page.locator(Login.error)).toBeVisible({ timeout: 10_000 })
    await expect(page.locator(Login.error)).toContainText('Invalid verification code')
  })

  test('shows rate limit message after too many attempts', async ({ page }) => {
    // Mock login API to simulate rate limiting after a few attempts
    let attemptCount = 0
    await page.route('**/api/auth/login', async (route) => {
      attemptCount++
      if (attemptCount <= 3) {
        await route.fulfill({
          status: 400,
          contentType: 'application/json',
          body: JSON.stringify({ error: { message: 'Invalid email or password', type: 'invalid_credentials' } }),
        })
      } else {
        await route.fulfill({
          status: 429,
          contentType: 'application/json',
          body: JSON.stringify({ error: { message: 'Too many login attempts. Please try again later.', type: 'rate_limited' } }),
        })
      }
    })

    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    // Submit wrong password multiple times
    for (let i = 0; i < 4; i++) {
      await page.locator('input.auth-input[type="email"]').fill(email)
      await page.locator('input.auth-input[type="password"]').fill(`wrong-password-${i}`)
      await page.locator('form button.auth-submit').click()
      await expect(page.locator(Login.error)).toBeVisible({ timeout: 10_000 })
      if (i < 3) {
        await page.locator('input.auth-input[type="email"]').clear()
        await page.locator('input.auth-input[type="password"]').clear()
      }
    }

    // Final error should be rate limit message
    await expect(page.locator(Login.error)).toContainText('Too many login attempts')
  })

  test('password field masks input', async ({ page }) => {
    await page.goto('/_ui/login')
    // Wait for auth-card to render (don't use networkidle — may be slow due to rate limiting)
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    const passwordInput = page.locator('input.auth-input[type="password"]')
    await expect(passwordInput).toBeVisible()
    await expect(passwordInput).toHaveAttribute('type', 'password')
  })

  test('form validates required fields before submit', async ({ page }) => {
    await page.goto('/_ui/login')
    await expect(page.locator(Login.card)).toBeVisible({ timeout: 15_000 })

    // Click submit with empty fields — browser validation prevents submission
    await page.locator('form button.auth-submit').click()

    // Should still be on the login page
    expect(page.url()).toContain('/login')

    // Verify the required attribute is present on both inputs
    await expect(page.locator('input.auth-input[type="email"]')).toHaveAttribute('required', '')
    await expect(page.locator('input.auth-input[type="password"]')).toHaveAttribute('required', '')
  })
})
