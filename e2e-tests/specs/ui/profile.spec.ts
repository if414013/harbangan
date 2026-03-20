import { test, expect } from '@playwright/test'
import { Card } from '../../helpers/selectors.js'
import { navigateTo } from '../../helpers/navigation.js'

test.describe('Profile page', () => {
  test('renders Account card with user info', async ({ page }) => {
    await navigateTo(page, '/profile')

    const accountTitle = page.locator(Card.title, { hasText: 'Account' })
    await expect(accountTitle).toBeVisible()
  })

  test('displays user role badge', async ({ page }) => {
    await navigateTo(page, '/profile')

    // Role badge next to account title (admin or user)
    const card = page.locator('div.card').first()
    const roleBadge = card.locator('.card-header span').last()
    await expect(roleBadge).toBeVisible()
  })

  test('API keys section renders', async ({ page }) => {
    await navigateTo(page, '/profile')

    const apiKeysHeader = page.locator('h2.section-header', { hasText: 'API KEYS' })
    await expect(apiKeysHeader).toBeVisible()
  })

  test('SECURITY section renders when auth is enabled', async ({ page }) => {
    await navigateTo(page, '/profile')

    // SECURITY section is conditional on auth methods being enabled
    // In test env, password auth is enabled so SECURITY should show
    const securityHeader = page.locator('h2.section-header', { hasText: 'SECURITY' })
    await expect(securityHeader).toBeVisible({ timeout: 10_000 })
  })

  test('page title shows profile', async ({ page }) => {
    await navigateTo(page, '/profile')

    await expect(page.locator('span.page-title')).toContainText('profile')
  })

  test('sidebar profile link is active', async ({ page }) => {
    await navigateTo(page, '/profile')

    const profileLink = page.locator('a.nav-link', { hasText: 'profile' })
    await expect(profileLink).toHaveClass(/active/)
  })
})
