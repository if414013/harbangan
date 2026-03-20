import { test, expect } from '@playwright/test'
import { Card } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

/**
 * UserDetail page E2E tests.
 * Runs in ui-admin project (workers: 1, serial).
 *
 * Navigates from admin page → click user row → verify detail page.
 * NOTE: Does NOT test delete (would break other tests using shared users).
 * Delete is covered in the user-management API test.
 */

test.describe('User detail page', () => {
  test.describe.serial('navigate and inspect', () => {
    let userId: string | null = null

    test('navigate from admin page to user detail', async ({ page }) => {
      await navigateTo(page, '/admin')

      // Wait for user table to load
      const usersTitle = page.locator(Card.title, { hasText: 'users' })
      await expect(usersTitle).toBeVisible({ timeout: 10_000 })

      // Click on the first user link in the table
      const userLink = page.locator('table.data-table tbody tr a').first()
      await expect(userLink).toBeVisible()

      // Capture the href to extract userId
      const href = await userLink.getAttribute('href')
      if (href) {
        const match = href.match(/\/admin\/users\/([a-f0-9-]+)/)
        if (match) userId = match[1]
      }

      await userLink.click()
      await page.waitForLoadState('networkidle')

      // Should be on user detail page
      expect(page.url()).toContain('/admin/users/')
    })

    test('detail page shows account card with user info', async ({ page }) => {
      // Navigate directly using the userId captured from previous test
      // or fall back to navigating via admin
      await navigateTo(page, '/admin')
      const userLink = page.locator('table.data-table tbody tr a').first()
      await expect(userLink).toBeVisible({ timeout: 10_000 })
      await userLink.click()
      await page.waitForLoadState('networkidle')

      // USER DETAIL section header
      const sectionHeader = page.locator('h2.section-header', { hasText: 'USER DETAIL' })
      await expect(sectionHeader).toBeVisible()

      // Account card
      const accountTitle = page.locator(Card.title, { hasText: 'account' })
      await expect(accountTitle).toBeVisible()

      // Role badge (clickable button)
      const roleBadge = page.locator('button.role-badge')
      await expect(roleBadge).toBeVisible()
      const roleText = await roleBadge.textContent()
      expect(['admin', 'user']).toContain(roleText?.trim())

      // User email displayed
      const email = page.locator('div', { hasText: /@/ }).last()
      await expect(email).toBeVisible()
    })

    test('detail page shows KIRO TOKEN section', async ({ page }) => {
      await navigateTo(page, '/admin')
      const userLink = page.locator('table.data-table tbody tr a').first()
      await expect(userLink).toBeVisible({ timeout: 10_000 })
      await userLink.click()
      await page.waitForLoadState('networkidle')

      const kiroHeader = page.locator('h2.section-header', { hasText: 'KIRO TOKEN' })
      await expect(kiroHeader).toBeVisible()

      // Status card
      const statusTitle = page.locator(Card.title, { hasText: 'status' })
      await expect(statusTitle).toBeVisible()
    })

    test('detail page shows API KEYS section', async ({ page }) => {
      await navigateTo(page, '/admin')
      const userLink = page.locator('table.data-table tbody tr a').first()
      await expect(userLink).toBeVisible({ timeout: 10_000 })
      await userLink.click()
      await page.waitForLoadState('networkidle')

      const keysHeader = page.locator('h2.section-header', { hasText: 'API KEYS' })
      await expect(keysHeader).toBeVisible()

      // Keys card shows count
      const keysTitle = page.locator(Card.title, { hasText: 'keys' })
      await expect(keysTitle).toBeVisible()
    })

    test('detail page has remove user button', async ({ page }) => {
      await navigateTo(page, '/admin')
      const userLink = page.locator('table.data-table tbody tr a').first()
      await expect(userLink).toBeVisible({ timeout: 10_000 })
      await userLink.click()
      await page.waitForLoadState('networkidle')

      const removeBtn = page.locator('button.device-code-cancel', { hasText: 'remove user' })
      await expect(removeBtn).toBeVisible()
    })

    test('back link navigates to admin page', async ({ page }) => {
      await navigateTo(page, '/admin')
      const userLink = page.locator('table.data-table tbody tr a').first()
      await expect(userLink).toBeVisible({ timeout: 10_000 })
      await userLink.click()
      await page.waitForLoadState('networkidle')

      // Click back link
      const backLink = page.locator('a', { hasText: '< back to admin' })
      await expect(backLink).toBeVisible()
      await backLink.click()
      await page.waitForLoadState('networkidle')

      expect(page.url()).toContain('/admin')
      // Should not contain /users/ anymore
      expect(page.url()).not.toContain('/users/')
    })
  })
})
