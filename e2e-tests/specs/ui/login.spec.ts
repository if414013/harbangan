import { test, expect } from '@playwright/test'
import { Login } from '../../helpers/selectors.js'

/** Mock the API calls the login page makes on mount. */
async function setupLoginMocks(page: import('@playwright/test').Page) {
  await page.route('**/api/auth/me', async (route) => {
    await route.fulfill({ status: 401, body: 'Unauthorized' })
  })
  await page.route('**/api/status', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        setup_complete: true,
        auth_google_enabled: true,
        auth_password_enabled: true,
      }),
    })
  })
}

test.describe('Login page', () => {
  test('renders auth card with sign-in buttons', async ({ page }) => {
    await setupLoginMocks(page)
    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    await expect(page.locator(Login.card)).toBeVisible()
    // Password form submit
    const passwordBtn = page.locator(Login.submit).filter({ hasText: '$ sign in' }).first()
    await expect(passwordBtn).toBeVisible()
    // Google sign-in button
    const googleBtn = page.locator(Login.submit).filter({ hasText: '$ sign in with google' })
    await expect(googleBtn).toBeVisible()
  })

  test('shows error for domain_not_allowed', async ({ page }) => {
    await setupLoginMocks(page)
    await page.goto('./login?error=domain_not_allowed')
    await page.waitForLoadState('networkidle')

    const error = page.locator(Login.error).first()
    await expect(error).toBeVisible()
    await expect(error).toHaveText('Your email domain is not authorized. Contact your admin.')
  })

  test('shows error for consent_denied', async ({ page }) => {
    await setupLoginMocks(page)
    await page.goto('./login?error=consent_denied')
    await page.waitForLoadState('networkidle')

    const error = page.locator(Login.error).first()
    await expect(error).toBeVisible()
    await expect(error).toHaveText('Google sign-in was cancelled.')
  })

  test('shows error for invalid_state', async ({ page }) => {
    await setupLoginMocks(page)
    await page.goto('./login?error=invalid_state')
    await page.waitForLoadState('networkidle')

    const error = page.locator(Login.error).first()
    await expect(error).toBeVisible()
    await expect(error).toHaveText('Login session expired. Please try again.')
  })

  test('shows error for auth_failed', async ({ page }) => {
    await setupLoginMocks(page)
    await page.goto('./login?error=auth_failed')
    await page.waitForLoadState('networkidle')

    const error = page.locator(Login.error).first()
    await expect(error).toBeVisible()
    await expect(error).toHaveText('Authentication failed. Please try again.')
  })
})
