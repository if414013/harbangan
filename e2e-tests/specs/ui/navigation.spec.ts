import { test, expect } from '@playwright/test'
import { Nav } from '../../helpers/selectors.js'
import { navigateTo } from '../../helpers/navigation.js'

test.describe('Navigation and layout', () => {
  test('layout renders sidebar, top-bar, and main content', async ({ page }) => {
    await navigateTo(page, '/profile')

    await expect(page.locator('nav.sidebar')).toBeAttached()
    await expect(page.locator('header.top-bar')).toBeVisible()
    await expect(page.locator('main#main-content')).toBeVisible()
  })

  test('sidebar nav links navigate and show active state', async ({ page }) => {
    await navigateTo(page, '/profile')

    // Profile link should be active
    const profileLink = page.locator(Nav.link, { hasText: 'profile' })
    await expect(profileLink).toHaveClass(/active/)

    // Click providers link
    const providersLink = page.locator(Nav.link, { hasText: 'providers' })
    await providersLink.click()
    await page.waitForLoadState('networkidle')
    await expect(providersLink).toHaveClass(/active/)
    expect(page.url()).toContain('/providers')
  })

  test('admin links are visible for admin user', async ({ page }) => {
    await navigateTo(page, '/profile')

    await expect(page.locator(Nav.link, { hasText: 'config' })).toBeVisible()
    await expect(page.locator(Nav.link, { hasText: 'guardrails' })).toBeVisible()
    await expect(page.locator(Nav.link, { hasText: 'admin' })).toBeVisible()
  })

  test('page title updates per route', async ({ page }) => {
    await navigateTo(page, '/profile')
    await expect(page.locator('span.page-title')).toContainText('profile')

    await navigateTo(page, '/config')
    await expect(page.locator('span.page-title')).toContainText('configuration')

    await navigateTo(page, '/admin')
    await expect(page.locator('span.page-title')).toContainText('administration')

    await navigateTo(page, '/guardrails')
    await expect(page.locator('span.page-title')).toContainText('guardrails')

    await navigateTo(page, '/providers')
    await expect(page.locator('span.page-title')).toContainText('providers')
  })

  test('logout button exists and is clickable', async ({ page }) => {
    await navigateTo(page, '/profile')

    const logoutBtn = page.locator('button.btn-logout', { hasText: '$ logout' })
    await logoutBtn.scrollIntoViewIfNeeded()
    await expect(logoutBtn).toBeVisible()
    await expect(logoutBtn).toBeEnabled()
  })
})
