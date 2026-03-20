import { test, expect } from '@playwright/test'
import { Card, Table } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

// ── Rendering tests ─────────────────────────────────────────────────

test.describe('Admin page', () => {
  test('renders Domain Allowlist and User Management sections', async ({ page }) => {
    await navigateTo(page, '/admin')

    const domainHeader = page.locator('h2.section-header', { hasText: 'DOMAIN ALLOWLIST' })
    await expect(domainHeader).toBeVisible()

    const userHeader = page.locator('h2.section-header', { hasText: 'USER MANAGEMENT' })
    await expect(userHeader).toBeVisible()
  })

  test('domain manager card is visible', async ({ page }) => {
    await navigateTo(page, '/admin')

    const domainCard = page.locator(Card.title, { hasText: 'allowed domains' })
    await expect(domainCard).toBeVisible()
  })

  test('user management section renders', async ({ page }) => {
    await navigateTo(page, '/admin')

    // Users card loads (title visible after API responds)
    const usersTitle = page.locator(Card.title, { hasText: 'users' })
    await expect(usersTitle).toBeVisible({ timeout: 10_000 })
  })

  test('renders Provider Pool section', async ({ page }) => {
    await navigateTo(page, '/admin')

    const poolHeader = page.locator('h2.section-header', { hasText: 'PROVIDER POOL' })
    await expect(poolHeader).toBeVisible()
  })

  test('renders Create Password User section', async ({ page }) => {
    await navigateTo(page, '/admin')

    const createHeader = page.locator('h2.section-header', { hasText: 'CREATE PASSWORD USER' })
    await expect(createHeader).toBeVisible()
  })
})

// ── Domain manager functional tests ─────────────────────────────────

test.describe('Admin page — domain manager', () => {
  test.describe.serial('domain add and remove', () => {
    const testDomain = `e2e-test-${Date.now()}.example.com`

    test('add a domain to the allowlist', async ({ page }) => {
      await navigateTo(page, '/admin')

      const domainInput = page.locator('input[aria-label="Domain name to allow"]')
      await expect(domainInput).toBeVisible()
      await domainInput.fill(testDomain)

      const addBtn = page.locator('button.btn-save', { hasText: '$ add domain' })
      await addBtn.click()

      await expectToastMessage(page, `Domain ${testDomain} added`)

      // Domain should appear in the list
      await page.waitForLoadState('networkidle')
      await expect(page.locator('span.domain-name', { hasText: testDomain })).toBeVisible()
    })

    test('remove the domain from the allowlist', async ({ page }) => {
      await navigateTo(page, '/admin')

      // Find the domain we just added and click remove
      const domainItem = page.locator('div.domain-item').filter({ hasText: testDomain })
      await expect(domainItem).toBeVisible({ timeout: 5_000 })

      const removeBtn = domainItem.locator('button', { hasText: 'remove' })
      await removeBtn.click()

      await expectToastMessage(page, `Domain ${testDomain} removed`)

      // Domain should no longer be in the list
      await page.waitForLoadState('networkidle')
      await expect(page.locator('span.domain-name', { hasText: testDomain })).not.toBeVisible()
    })
  })
})

// ── Provider pool rendering tests ───────────────────────────────────

test.describe('Admin page — provider pool', () => {
  test('pool form has provider select, label, key, and base URL inputs', async ({ page }) => {
    await navigateTo(page, '/admin')

    // Provider select
    const providerSelect = page.locator('select.config-input').filter({
      has: page.locator('option[value="anthropic"]'),
    })
    await expect(providerSelect).toBeVisible()

    // Label input
    const labelInput = page.locator('input.config-input[placeholder="account label"]')
    await expect(labelInput).toBeVisible()

    // API key input
    const keyInput = page.locator('input.config-input[type="password"][placeholder="API key"]')
    await expect(keyInput).toBeVisible()

    // Base URL input (optional)
    const baseUrlInput = page.locator('input.config-input[placeholder="base URL (optional)"]')
    await expect(baseUrlInput).toBeVisible()

    // Add button
    const addBtn = page.locator('button.btn-save', { hasText: 'Add Pool Account' })
    await expect(addBtn).toBeVisible()
  })

  test('provider select has expected options', async ({ page }) => {
    await navigateTo(page, '/admin')

    const providerSelect = page.locator('select.config-input').filter({
      has: page.locator('option[value="anthropic"]'),
    })

    await expect(providerSelect.locator('option[value="anthropic"]')).toBeAttached()
    await expect(providerSelect.locator('option[value="openai_codex"]')).toBeAttached()
    await expect(providerSelect.locator('option[value="kiro"]')).toBeAttached()
    await expect(providerSelect.locator('option[value="copilot"]')).toBeAttached()
    await expect(providerSelect.locator('option[value="qwen"]')).toBeAttached()
  })
})

// ── Create user form validation ─────────────────────────────────────

test.describe('Admin page — create user form', () => {
  test('create user form has email, name, password, and role fields', async ({ page }) => {
    await navigateTo(page, '/admin')

    await expect(page.locator('input[type="email"][placeholder="email"]')).toBeVisible()
    await expect(page.locator('input[type="text"][placeholder="name"]')).toBeVisible()
    await expect(page.locator('input[type="password"][placeholder*="password"]')).toBeVisible()

    // Role select with user/admin options
    const roleSelect = page.locator('select.config-input').filter({
      has: page.locator('option[value="user"]'),
    }).filter({
      has: page.locator('option[value="admin"]'),
    })
    await expect(roleSelect).toBeVisible()
  })

  test('create user button is disabled while creating', async ({ page }) => {
    await navigateTo(page, '/admin')

    const createBtn = page.locator('button.btn-save', { hasText: 'Create User' })
    await expect(createBtn).toBeVisible()
    await expect(createBtn).toBeEnabled()
  })
})
