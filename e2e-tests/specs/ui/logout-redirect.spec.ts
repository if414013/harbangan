import { test, expect, type BrowserContext } from '@playwright/test'
import { Nav } from '../../helpers/selectors.js'
import { adminLogin } from '../../helpers/auth.js'

/**
 * Logout redirect E2E tests.
 *
 * CRITICAL: This test creates its OWN fresh session via adminLogin() in beforeAll.
 * It does NOT use the shared storageState because logout invalidates the session,
 * which would break all subsequent tests that rely on the shared admin session.
 *
 * Runs in ui-admin project (workers: 1, serial).
 */

const BASE_UI_URL = process.env.BASE_UI_URL || 'http://localhost:5173/_ui'
const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:9999'

test.describe('Logout redirect', () => {
  let context: BrowserContext

  test.beforeAll(async ({ browser }) => {
    // Create a fresh session — NOT using the shared storageState
    const storageState = await adminLogin(GATEWAY_URL)
    context = await browser.newContext({
      baseURL: BASE_UI_URL,
      ignoreHTTPSErrors: true,
      storageState,
    })
  })

  test.afterAll(async () => {
    await context?.close()
  })

  test('clicking logout redirects to login page', async () => {
    const page = await context.newPage()
    try {
      // Navigate to a protected page
      await page.goto('./profile')
      await page.waitForLoadState('networkidle')

      // Verify we're on a protected page (sidebar visible)
      await expect(page.locator('nav.sidebar')).toBeAttached()

      // Click the logout button
      const logoutBtn = page.locator(Nav.logout)
      await expect(logoutBtn).toBeVisible()
      await logoutBtn.click()

      // Should redirect to login page
      await page.waitForURL('**/login**', { timeout: 10_000 })
      expect(page.url()).toContain('/login')
    } finally {
      await page.close()
    }
  })

  test('accessing protected page after logout redirects to login', async () => {
    const page = await context.newPage()
    try {
      // Try to navigate to a protected page with the now-invalidated session
      await page.goto('./profile')
      await page.waitForLoadState('networkidle')

      // Should redirect to login since session was invalidated
      await page.waitForURL('**/login**', { timeout: 10_000 })
      expect(page.url()).toContain('/login')
    } finally {
      await page.close()
    }
  })
})
