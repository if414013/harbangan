import { test, expect } from '@playwright/test'
import { Card } from '../../helpers/selectors.js'
import { navigateTo } from '../../helpers/navigation.js'

test.describe('Index redirect', () => {
  test('index redirects to profile page', async ({ page }) => {
    await page.goto('./')
    await page.waitForURL(/\/profile/, { timeout: 10_000 })
    expect(page.url()).toContain('/profile')
  })

  test('profile page renders after index redirect', async ({ page }) => {
    await page.goto('./')
    await page.waitForURL(/\/profile/, { timeout: 10_000 })

    await expect(page.locator('span.page-title')).toContainText('profile')
    const accountTitle = page.locator(Card.title, { hasText: 'Account' })
    await expect(accountTitle).toBeVisible()
  })

  test('API keys section visible after redirect', async ({ page }) => {
    await page.goto('./')
    await page.waitForURL(/\/profile/, { timeout: 10_000 })

    const apiKeysHeader = page.locator('h2.section-header', { hasText: 'API KEYS' })
    await expect(apiKeysHeader).toBeVisible()
  })

  test('sidebar is accessible after redirect', async ({ page }) => {
    await page.goto('./')
    await page.waitForURL(/\/profile/, { timeout: 10_000 })

    await expect(page.locator('nav.sidebar')).toBeAttached()
    const profileLink = page.locator('a.nav-link', { hasText: 'profile' })
    await expect(profileLink).toHaveClass(/active/)
  })
})
