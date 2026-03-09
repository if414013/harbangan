import { test, expect } from '@playwright/test'
import { Card, Form, Table } from '../../helpers/selectors.js'
import { navigateTo } from '../../helpers/navigation.js'

test.describe('Models page', () => {
  test('renders MODEL REGISTRY section header', async ({ page }) => {
    await navigateTo(page, '/models')

    const header = page.locator('h2.section-header', { hasText: 'MODEL REGISTRY' })
    await expect(header).toBeVisible()
  })

  test('populate all button is visible', async ({ page }) => {
    await navigateTo(page, '/models')

    const populateBtn = page.locator(Form.save, { hasText: '$ populate all' })
    await expect(populateBtn).toBeVisible()
  })

  test('shows model count summary', async ({ page }) => {
    await navigateTo(page, '/models')

    const summary = page.locator('.card-subtitle')
    await expect(summary).toBeVisible()
    await expect(summary).toContainText('models across')
    await expect(summary).toContainText('providers')
  })

  test('shows empty state when no models', async ({ page }) => {
    await navigateTo(page, '/models')

    // Either models are loaded (config-group sections) or empty state is shown
    const emptyState = page.locator('.empty-state', { hasText: 'No models in registry' })
    const providerGroup = page.locator('.config-group').first()

    const hasModels = await providerGroup.isVisible().catch(() => false)
    if (!hasModels) {
      await expect(emptyState).toBeVisible()
    }
  })

  test('provider sections are collapsible', async ({ page }) => {
    await navigateTo(page, '/models')

    const group = page.locator('.config-group').first()
    const isVisible = await group.isVisible().catch(() => false)
    if (!isVisible) return // no models populated yet

    const header = group.locator('.config-group-header')
    await expect(header).toBeVisible()

    // Click to collapse
    await header.click()
    await expect(group).toHaveClass(/collapsed/)

    // Click to expand
    await header.click()
    await expect(group).not.toHaveClass(/collapsed/)
  })

  test('provider section shows enabled count', async ({ page }) => {
    await navigateTo(page, '/models')

    const group = page.locator('.config-group').first()
    const isVisible = await group.isVisible().catch(() => false)
    if (!isVisible) return

    const header = group.locator('.config-group-header')
    await expect(header).toContainText('enabled')
  })

  test('provider section has populate, enable all, disable all buttons', async ({ page }) => {
    await navigateTo(page, '/models')

    const group = page.locator('.config-group').first()
    const isVisible = await group.isVisible().catch(() => false)
    if (!isVisible) return

    await expect(group.locator('.btn-reveal', { hasText: '$ populate' })).toBeVisible()
    await expect(group.locator('.btn-reveal', { hasText: 'enable all' })).toBeVisible()
    await expect(group.locator('.btn-reveal', { hasText: 'disable all' })).toBeVisible()
  })

  test('model table has correct columns', async ({ page }) => {
    await navigateTo(page, '/models')

    const table = page.locator(Table.dataTable).first()
    const isVisible = await table.isVisible().catch(() => false)
    if (!isVisible) return

    await expect(table.locator('th', { hasText: 'enabled' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'prefixed id' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'display name' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'context' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'source' })).toBeVisible()
  })

  test('model rows have toggle and delete buttons', async ({ page }) => {
    await navigateTo(page, '/models')

    const table = page.locator(Table.dataTable).first()
    const isVisible = await table.isVisible().catch(() => false)
    if (!isVisible) return

    const firstRow = table.locator('tbody tr').first()
    await expect(firstRow.locator('.role-badge')).toBeVisible()
    await expect(firstRow.locator('.device-code-cancel', { hasText: 'delete' })).toBeVisible()
  })

  test('navigate to models via sidebar', async ({ page }) => {
    await navigateTo(page, '/profile')

    const modelsLink = page.locator('a.nav-link', { hasText: 'models' })
    await expect(modelsLink).toBeVisible()
    await modelsLink.click()

    await expect(page).toHaveURL(/\/models/)
    const header = page.locator('h2.section-header', { hasText: 'MODEL REGISTRY' })
    await expect(header).toBeVisible()
  })
})
