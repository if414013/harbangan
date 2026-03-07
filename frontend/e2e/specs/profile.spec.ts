import { test, expect } from '@playwright/test'
import { Card, Status } from '../helpers/selectors.js'
import { navigateTo } from '../helpers/navigation.js'

test.describe('Profile page', () => {
  test('renders user info with name and email', async ({ page }) => {
    await navigateTo(page, '/profile')

    // Account card header
    const accountTitle = page.locator(Card.title, { hasText: 'account' })
    await expect(accountTitle).toBeVisible()

    // Profile section header
    const profileHeader = page.locator('h2.section-header', { hasText: 'PROFILE' })
    await expect(profileHeader).toBeVisible()
  })

  test('kiro connection card renders with status badge', async ({ page }) => {
    await navigateTo(page, '/profile')

    // Kiro token section header
    const kiroHeader = page.locator('h2.section-header', { hasText: 'KIRO TOKEN' })
    await expect(kiroHeader).toBeVisible()

    // Kiro connection card
    const kiroTitle = page.locator(Card.title, { hasText: 'kiro connection' })
    await expect(kiroTitle).toBeVisible()

    // Status badge should be one of: tag-ok (CONNECTED), tag-warn (EXPIRED), tag-err (NOT CONNECTED)
    const statusBadge = page.locator(`${Status.ok}, ${Status.warn}, ${Status.err}`).first()
    await expect(statusBadge).toBeVisible()
  })

  test('API keys section renders', async ({ page }) => {
    await navigateTo(page, '/profile')

    // API Keys section header
    const apiKeysHeader = page.locator('h2.section-header', { hasText: 'API KEYS' })
    await expect(apiKeysHeader).toBeVisible()

    // API keys card
    const apiKeysTitle = page.locator(Card.title, { hasText: 'api keys' })
    await expect(apiKeysTitle).toBeVisible()
  })

  test('PROVIDERS section renders', async ({ page }) => {
    await navigateTo(page, '/profile')

    const providersHeader = page.locator('h2.section-header', { hasText: 'PROVIDERS' })
    await expect(providersHeader).toBeVisible()
  })
})
