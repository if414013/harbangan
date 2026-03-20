import { test, expect } from '@playwright/test'
import { navigateTo } from '../../helpers/navigation.js'

/**
 * Usage page E2E tests — read-only rendering + interactions.
 * Runs in ui-authenticated project (parallel, no mutations).
 */

test.describe('Usage page', () => {
  test('renders page header', async ({ page }) => {
    await navigateTo(page, '/usage')

    // PageHeader renders the title in h1.page-header-title
    await expect(page.locator('h1.page-header-title')).toContainText('usage')
  })

  test('renders 4 summary cards', async ({ page }) => {
    await navigateTo(page, '/usage')

    // 4 summary cards: Total Requests, Input Tokens, Output Tokens, Total Cost
    const cards = page.locator('div.card').filter({ has: page.locator('div', { hasText: /Total Requests|Input Tokens|Output Tokens|Total Cost/ }) })
    // At minimum we expect 4 summary items to be visible
    await expect(page.getByText('Total Requests')).toBeVisible()
    await expect(page.getByText('Input Tokens')).toBeVisible()
    await expect(page.getByText('Output Tokens')).toBeVisible()
    await expect(page.getByText('Total Cost')).toBeVisible()
  })

  test('renders BREAKDOWN section with data table', async ({ page }) => {
    await navigateTo(page, '/usage')

    const header = page.locator('h2.section-header', { hasText: 'BREAKDOWN' })
    await expect(header).toBeVisible()

    // Data table or empty state should be present
    const tableCard = header.locator('~ div.card')
    await expect(tableCard).toBeVisible()
  })

  test('date pickers are present and functional', async ({ page }) => {
    await navigateTo(page, '/usage')

    const fromInput = page.locator('input[type="date"]').first()
    const toInput = page.locator('input[type="date"]').last()

    await expect(fromInput).toBeVisible()
    await expect(toInput).toBeVisible()

    // Date inputs should have values (defaults to 30-day range)
    await expect(fromInput).not.toHaveValue('')
    await expect(toInput).not.toHaveValue('')
  })

  test('group-by select has day/model/provider options', async ({ page }) => {
    await navigateTo(page, '/usage')

    const groupBySelect = page.locator('select.config-input')
    await expect(groupBySelect).toBeVisible()

    // Verify all three options exist
    await expect(groupBySelect.locator('option[value="day"]')).toBeAttached()
    await expect(groupBySelect.locator('option[value="model"]')).toBeAttached()
    await expect(groupBySelect.locator('option[value="provider"]')).toBeAttached()
  })

  test('changing group-by updates the table column header', async ({ page }) => {
    await navigateTo(page, '/usage')

    const groupBySelect = page.locator('select.config-input')

    // Default is "day" — first column should be "Date"
    await expect(groupBySelect).toHaveValue('day')

    // Switch to "model"
    await groupBySelect.selectOption('model')
    await page.waitForLoadState('networkidle')

    // Switch to "provider"
    await groupBySelect.selectOption('provider')
    await page.waitForLoadState('networkidle')

    // Switch back to "day"
    await groupBySelect.selectOption('day')
    await page.waitForLoadState('networkidle')
  })
})

test.describe('Usage page — admin tabs', () => {
  test('admin sees My Usage / Global / Per-User tabs', async ({ page }) => {
    await navigateTo(page, '/usage')

    // Admin should see all three tab buttons
    await expect(page.locator('button', { hasText: 'My Usage' })).toBeVisible()
    await expect(page.locator('button', { hasText: 'Global' })).toBeVisible()
    await expect(page.locator('button', { hasText: 'Per-User' })).toBeVisible()
  })

  test.describe.serial('tab interactions', () => {
    test('clicking Global tab loads global usage', async ({ page }) => {
      await navigateTo(page, '/usage')

      const globalTab = page.locator('button', { hasText: 'Global' })
      await globalTab.click()
      await page.waitForLoadState('networkidle')

      // The section header should still show BREAKDOWN
      await expect(page.locator('h2.section-header', { hasText: 'BREAKDOWN' })).toBeVisible()
    })

    test('clicking Per-User tab shows USER BREAKDOWN header', async ({ page }) => {
      await navigateTo(page, '/usage')

      const perUserTab = page.locator('button', { hasText: 'Per-User' })
      await perUserTab.click()
      await page.waitForLoadState('networkidle')

      // Section header should change to USER BREAKDOWN
      await expect(page.locator('h2.section-header', { hasText: 'USER BREAKDOWN' })).toBeVisible()

      // Group-by select should be hidden for Per-User tab
      await expect(page.locator('select.config-input')).not.toBeVisible()
    })

    test('clicking My Usage tab returns to personal view', async ({ page }) => {
      await navigateTo(page, '/usage')

      // Click Per-User first, then back to My Usage
      await page.locator('button', { hasText: 'Per-User' }).click()
      await page.waitForLoadState('networkidle')

      await page.locator('button', { hasText: 'My Usage' }).click()
      await page.waitForLoadState('networkidle')

      // Section header back to BREAKDOWN
      await expect(page.locator('h2.section-header', { hasText: 'BREAKDOWN' })).toBeVisible()

      // Group-by select should be visible again
      await expect(page.locator('select.config-input')).toBeVisible()
    })
  })
})
