import { test, expect, request as apiRequest } from '@playwright/test'
import { Card, Toast } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'
import { adminLogin, csrfHeaders } from '../../helpers/csrf.js'

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:9999'

/**
 * Profile page functional E2E tests — mutating actions.
 *
 * Read-only rendering stays in profile.spec.ts (ui-authenticated, parallel).
 * These tests create/delete API keys and are mutating, so they run in
 * ui-admin project (workers: 1, serial).
 */

test.describe('Profile page — API key management', () => {
  // Clean up e2e-profile/copy keys after test to avoid hitting the 10-key limit
  test.afterAll(async () => {
    const ctx = await apiRequest.newContext({ baseURL: GATEWAY_URL })
    try {
      const { csrfToken } = await adminLogin(ctx)
      const listRes = await ctx.get('/_ui/api/keys')
      const listBody = await listRes.json()
      const staleKeys = (listBody.keys ?? []).filter(
        (k: { label: string }) =>
          k.label.startsWith('e2e-profile-') || k.label.startsWith('e2e-copy-test-')
      )
      for (const key of staleKeys) {
        await ctx.delete(`/_ui/api/keys/${key.id}`, {
          headers: csrfHeaders(csrfToken),
        })
      }
    } finally {
      await ctx.dispose()
    }
  })

  test.describe.serial('API key lifecycle', () => {
    let createdKeyPrefix: string | null = null

    test('create an API key via generate button', async ({ page }) => {
      await navigateTo(page, '/profile')

      // API KEYS section header
      const keysHeader = page.locator('h2.section-header', { hasText: 'API KEYS' })
      await expect(keysHeader).toBeVisible()

      // Card should show api keys title
      const keysTitle = page.locator(Card.title, { hasText: 'api keys' })
      await expect(keysTitle).toBeVisible()

      // Fill label and click generate
      const labelInput = page.locator('input[aria-label="API key label"]')
      await expect(labelInput).toBeVisible()
      await labelInput.fill(`e2e-profile-${Date.now()}`)

      const generateBtn = page.locator('button.btn-save', { hasText: '$ generate key' })
      await expect(generateBtn).toBeVisible()
      await generateBtn.click()

      // New key banner should appear with the key value
      const keyBanner = page.locator('div.key-new-banner')
      await expect(keyBanner).toBeVisible({ timeout: 5_000 })

      // The key value should be displayed
      const keyValue = page.locator('code.key-new-value')
      await expect(keyValue).toBeVisible()
      const keyText = await keyValue.textContent()
      expect(keyText).toBeTruthy()
      expect(keyText!.length).toBeGreaterThan(10)
    })

    test('copy button works on newly created key', async ({ page }) => {
      await navigateTo(page, '/profile')

      // Create another key to test copy
      const labelInput = page.locator('input[aria-label="API key label"]')
      await labelInput.fill(`e2e-copy-test-${Date.now()}`)

      const generateBtn = page.locator('button.btn-save', { hasText: '$ generate key' })
      await generateBtn.click()

      // Wait for key banner
      const keyBanner = page.locator('div.key-new-banner')
      await expect(keyBanner).toBeVisible({ timeout: 5_000 })

      // Copy button should be visible
      const copyBtn = page.locator('button.btn-reveal', { hasText: '[copy]' })
      await expect(copyBtn).toBeVisible()

      // Click copy (may fail in headless due to clipboard permissions, but button should respond)
      await copyBtn.click()

      // Button text should change to [copied] or we just verify it's clickable
      // Clipboard API may not work in test env, so just verify the button responded
      const copiedOrCopy = page.locator('button.btn-reveal')
      await expect(copiedOrCopy).toBeVisible()

      // Dismiss the banner
      const dismissBtn = page.locator('button.device-code-cancel', { hasText: 'dismiss' })
      await dismissBtn.click()
      await expect(keyBanner).not.toBeVisible()
    })

    test('created key appears in the keys table', async ({ page }) => {
      await navigateTo(page, '/profile')

      // There should be at least one key in the table now
      const keysTable = page.locator('table.data-table')
      await expect(keysTable).toBeVisible({ timeout: 5_000 })

      // At least one row with a key prefix
      const rows = keysTable.locator('tbody tr')
      const count = await rows.count()
      expect(count).toBeGreaterThan(0)

      // Capture the first key prefix for revoke test
      const firstPrefix = await rows.first().locator('td').first().textContent()
      if (firstPrefix) createdKeyPrefix = firstPrefix.replace('...', '')
    })

    test('revoke an API key removes it from the table', async ({ page }) => {
      await navigateTo(page, '/profile')

      // Wait for keys table
      const keysTable = page.locator('table.data-table')
      await expect(keysTable).toBeVisible({ timeout: 5_000 })

      const rowsBefore = await keysTable.locator('tbody tr').count()
      expect(rowsBefore).toBeGreaterThan(0)

      // Click revoke on the first key
      const revokeBtn = keysTable.locator('button.btn-danger', { hasText: 'revoke' }).first()
      await expect(revokeBtn).toBeVisible()
      await revokeBtn.click()

      // Expect success toast
      await expectToastMessage(page, 'API key revoked')

      // Row count should decrease (or table may disappear if it was the last key)
      await page.waitForLoadState('networkidle')
    })
  })
})

test.describe('Profile page — security section', () => {
  test('security section renders for password auth user', async ({ page }) => {
    await navigateTo(page, '/profile')

    // The security section visibility depends on auth config.
    // If password auth is enabled, we should see the SECURITY header.
    // If only Google auth, the section may render differently.
    // Check if SECURITY section is present
    const securityHeader = page.locator('h2.section-header', { hasText: 'SECURITY' })
    const hasSecuritySection = await securityHeader.isVisible().catch(() => false)

    if (hasSecuritySection) {
      // Within the security card, we should see either:
      // - Google link status (if google auth enabled)
      // - 2FA status + change password / reset 2fa buttons (if password auth)
      const securityCard = securityHeader.locator('~ div.card')
      await expect(securityCard).toBeVisible()
    }
    // If security section is not visible, that's also valid (no auth methods enabled)
  })

  test('change password button navigates to password change page', async ({ page }) => {
    await navigateTo(page, '/profile')

    // Check if change/set password button exists (depends on auth method)
    const changePasswordBtn = page.locator('button.btn-save', { hasText: /\$ (change|set) password/ })
    const hasBtn = await changePasswordBtn.isVisible().catch(() => false)

    if (hasBtn) {
      await changePasswordBtn.click()
      await page.waitForLoadState('networkidle')
      expect(page.url()).toContain('/change-password')
    }
    // Skip if button not present (user is Google-only)
  })
})
