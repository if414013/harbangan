import { test, expect } from '@playwright/test'

test.describe('Unauthenticated redirects', () => {
  test('/ redirects to /login', async ({ page }) => {
    await page.goto('./')
    await page.waitForLoadState('networkidle')
    expect(page.url()).toContain('/login')
  })

  test('/profile redirects to /login', async ({ page }) => {
    await page.goto('./profile')
    await page.waitForLoadState('networkidle')
    expect(page.url()).toContain('/login')
  })

  test('/config redirects to /login', async ({ page }) => {
    await page.goto('./config')
    await page.waitForLoadState('networkidle')
    expect(page.url()).toContain('/login')
  })

  test('/admin redirects to /login', async ({ page }) => {
    await page.goto('./admin')
    await page.waitForLoadState('networkidle')
    expect(page.url()).toContain('/login')
  })
})
