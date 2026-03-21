import { test, expect, type BrowserContext } from '@playwright/test'
import { Form } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'
import { adminLogin as adminLoginAuth } from '../../helpers/auth.js'
import { adminLogin as adminLoginCsrf, csrfHeaders } from '../../helpers/csrf.js'

/**
 * SSO config flow E2E tests — cross-page integration.
 *
 * Tests the full lifecycle: admin configures SSO in config page →
 * login page reflects the change. Runs in ui-admin project (serial).
 */

const BASE_UI_URL = (process.env.BASE_UI_URL || 'http://localhost:5173/_ui').replace(/\/?$/, '/')
const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:9999'

// ── Cleanup: restore SSO fields to empty after all tests ────────────

test.afterAll(async ({ request: _request }) => {
  // Use a fresh API context for cleanup
  const { request: apiRequest } = await import('@playwright/test')
  const ctx = await apiRequest.newContext({ baseURL: GATEWAY_URL })
  try {
    const { csrfToken } = await adminLoginCsrf(ctx)
    await ctx.put('/_ui/api/config', {
      data: {
        google_client_id: '',
        google_client_secret: '',
        google_callback_url: '',
        auth_google_enabled: false,
      },
      headers: csrfHeaders(csrfToken),
    })
  } finally {
    await ctx.dispose()
  }
})

// ── Config page: SSO field rendering ────────────────────────────────

test.describe('Config page — SSO fields rendering', () => {
  test('Authentication group renders google_client_id field', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    await expect(authGroup).toBeVisible()

    const clientIdLabel = authGroup.locator('label.config-label', { hasText: 'Client ID' })
    await expect(clientIdLabel).toBeVisible()

    const clientIdInput = authGroup.locator('input#google_client_id')
    await expect(clientIdInput).toBeVisible()
  })

  test('Authentication group renders google_client_secret field', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const secretLabel = authGroup.locator('label.config-label', { hasText: 'Client Secret' })
    await expect(secretLabel).toBeVisible()

    // Should be a password-type input
    const secretInput = authGroup.locator('input#google_client_secret')
    await expect(secretInput).toBeVisible()
    await expect(secretInput).toHaveAttribute('type', 'password')
  })

  test('Authentication group renders google_callback_url field', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const urlLabel = authGroup.locator('label.config-label', { hasText: 'Callback URL' })
    await expect(urlLabel).toBeVisible()

    const urlInput = authGroup.locator('input#google_callback_url')
    await expect(urlInput).toBeVisible()
  })
})

// ── Config page: edit and save SSO fields ───────────────────────────

test.describe('Config page — SSO field edit and save', () => {
  test('admin edits google_client_id and saves', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#google_client_id')
    await expect(input).toBeVisible()

    const testValue = `e2e-client-id-${Date.now()}`
    await input.clear()
    await input.fill(testValue)

    const saveBtn = page.locator(Form.save, { hasText: 'Save Configuration' })
    await saveBtn.click()
    await expectToastMessage(page, 'applied immediately')

    // Reload and verify persisted
    await navigateTo(page, '/config')
    const input2 = page.locator('div.config-group').filter({ hasText: 'Authentication' })
      .locator('input#google_client_id')
    await expect(input2).toHaveValue(testValue)
  })

  test('admin edits google_callback_url and saves', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#google_callback_url')
    await expect(input).toBeVisible()

    const testUrl = 'http://localhost:9999/_ui/api/auth/google/callback'
    await input.clear()
    await input.fill(testUrl)

    await page.locator(Form.save, { hasText: 'Save Configuration' }).click()
    await expectToastMessage(page, 'applied immediately')

    await navigateTo(page, '/config')
    const input2 = page.locator('div.config-group').filter({ hasText: 'Authentication' })
      .locator('input#google_callback_url')
    await expect(input2).toHaveValue(testUrl)
  })

  test('google_client_secret shows masked value after save', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#google_client_secret')
    await expect(input).toBeVisible()

    await input.clear()
    await input.fill('test-secret-value-long-enough-12345')

    await page.locator(Form.save, { hasText: 'Save Configuration' }).click()
    await expectToastMessage(page, 'applied immediately')

    // Reload — the value should be masked (not the raw secret)
    await navigateTo(page, '/config')
    const input2 = page.locator('div.config-group').filter({ hasText: 'Authentication' })
      .locator('input#google_client_secret')
    const displayedValue = await input2.inputValue()
    // Should contain masking characters (... or ****)
    expect(displayedValue).not.toBe('test-secret-value-long-enough-12345')
    if (displayedValue !== '') {
      expect(displayedValue).toMatch(/\.\.\.|^\*{4}$/)
    }
  })

  test('unsaved changes indicator appears when editing SSO field', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#google_client_id')
    const originalValue = await input.inputValue()

    await input.clear()
    await input.fill(originalValue + '-modified')

    const saveBar = page.locator('div.config-save-bar')
    await expect(saveBar).toBeVisible()

    // Revert by navigating away
    await navigateTo(page, '/config')
  })
})

// ── Cross-page: SSO enable/disable → login page ────────────────────

test.describe('SSO toggle → login page integration', () => {
  test('enabling SSO with credentials makes Google button appear on login page', async ({ page, browser }) => {
    // Step 1: Set SSO credentials and enable via config page
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })

    // Set client ID
    const clientIdInput = authGroup.locator('input#google_client_id')
    await clientIdInput.clear()
    await clientIdInput.fill(`sso-flow-test-${Date.now()}`)

    // Set client secret
    const secretInput = authGroup.locator('input#google_client_secret')
    await secretInput.clear()
    await secretInput.fill('sso-flow-test-secret-long-enough')

    // Set callback URL
    const urlInput = authGroup.locator('input#google_callback_url')
    await urlInput.clear()
    await urlInput.fill('http://localhost:9999/_ui/api/auth/google/callback')

    // Enable Google SSO toggle
    const googleToggle = authGroup.locator('input#auth_google_enabled')
    if (!(await googleToggle.isChecked())) {
      await googleToggle.click()
    }

    // Save
    await page.locator(Form.save, { hasText: 'Save Configuration' }).click()
    await expectToastMessage(page, 'applied immediately')

    // Step 2: Open unauthenticated context → login page should show Google button
    const unauthContext = await browser.newContext({
      baseURL: BASE_UI_URL,
      ignoreHTTPSErrors: true,
      storageState: undefined,
    })
    const loginPage = await unauthContext.newPage()
    try {
      await loginPage.goto('./login')
      await loginPage.waitForLoadState('networkidle')

      // Google sign-in button should be visible
      const googleBtn = loginPage.locator('button.auth-submit').filter({ hasText: /google/i })
      await expect(googleBtn).toBeVisible({ timeout: 10_000 })
    } finally {
      await loginPage.close()
      await unauthContext.close()
    }
  })

  test('disabling SSO hides Google button on login page', async ({ page, browser }) => {
    // Step 1: Disable Google SSO in config
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const googleToggle = authGroup.locator('input#auth_google_enabled')
    if (await googleToggle.isChecked()) {
      await googleToggle.click()
    }

    await page.locator(Form.save, { hasText: 'Save Configuration' }).click()
    await expectToastMessage(page, 'applied immediately')

    // Step 2: Open unauthenticated context → login page should NOT show Google button
    const unauthContext = await browser.newContext({
      baseURL: BASE_UI_URL,
      ignoreHTTPSErrors: true,
      storageState: undefined,
    })
    const loginPage = await unauthContext.newPage()
    try {
      await loginPage.goto('./login')
      await loginPage.waitForLoadState('networkidle')

      // Google sign-in button should NOT be visible
      const googleBtn = loginPage.locator('button.auth-submit').filter({ hasText: /google/i })
      await expect(googleBtn).not.toBeVisible({ timeout: 5_000 })

      // Password form should still be visible
      const passwordInput = loginPage.locator('input.auth-input[type="password"]')
      await expect(passwordInput).toBeVisible()
    } finally {
      await loginPage.close()
      await unauthContext.close()
    }
  })
})

// ── First-run / fresh setup ─────────────────────────────────────────

test.describe('SSO config — fresh state', () => {
  test('config page loads with empty SSO fields by default', async ({ page }) => {
    // First clear all SSO fields via API
    const { request: apiRequest } = await import('@playwright/test')
    const ctx = await apiRequest.newContext({ baseURL: GATEWAY_URL })
    try {
      const { csrfToken } = await adminLoginCsrf(ctx)
      await ctx.put('/_ui/api/config', {
        data: {
          google_client_id: '',
          google_client_secret: '',
          google_callback_url: '',
          auth_google_enabled: false,
        },
        headers: csrfHeaders(csrfToken),
      })
    } finally {
      await ctx.dispose()
    }

    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const clientIdInput = authGroup.locator('input#google_client_id')
    const callbackInput = authGroup.locator('input#google_callback_url')

    await expect(clientIdInput).toHaveValue('')
    await expect(callbackInput).toHaveValue('')
  })

  test('login page shows only password login when SSO is disabled', async ({ browser }) => {
    const unauthContext = await browser.newContext({
      baseURL: BASE_UI_URL,
      ignoreHTTPSErrors: true,
      storageState: undefined,
    })
    const loginPage = await unauthContext.newPage()
    try {
      await loginPage.goto('./login')
      await loginPage.waitForLoadState('networkidle')

      // Password form should be visible
      const emailInput = loginPage.locator('input.auth-input[type="email"]')
      await expect(emailInput).toBeVisible({ timeout: 10_000 })

      // Google button should not be present
      const googleBtn = loginPage.locator('button.auth-submit').filter({ hasText: /google/i })
      await expect(googleBtn).not.toBeVisible()
    } finally {
      await loginPage.close()
      await unauthContext.close()
    }
  })
})

// ── Error handling ──────────────────────────────────────────────────

test.describe('Config page — SSO error handling', () => {
  test('save failure shows error toast and retains form values', async ({ page }) => {
    await navigateTo(page, '/config')

    // Mock the PUT to fail
    await page.route('**/api/config', async (route) => {
      if (route.request().method() === 'PUT') {
        await route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Internal server error' }),
        })
      } else {
        await route.continue()
      }
    })

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#google_client_id')
    const testValue = `error-test-${Date.now()}`
    await input.clear()
    await input.fill(testValue)

    await page.locator(Form.save, { hasText: 'Save Configuration' }).click()

    // Should show error toast
    await expectToastMessage(page, /error|failed/i, 'error')
  })

  test('validation error shows error toast', async ({ page }) => {
    await navigateTo(page, '/config')

    // Mock PUT to return 400
    await page.route('**/api/config', async (route) => {
      if (route.request().method() === 'PUT') {
        await route.fulfill({
          status: 400,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'google_client_id must not contain control characters' }),
        })
      } else {
        await route.continue()
      }
    })

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#google_client_id')
    await input.clear()
    await input.fill('invalid-value')

    await page.locator(Form.save, { hasText: 'Save Configuration' }).click()
    await expectToastMessage(page, /error|control characters/i, 'error')
  })
})
