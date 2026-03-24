import { test, expect } from '@playwright/test'
import { Card } from '../../helpers/selectors.js'
import { navigateTo } from '../../helpers/navigation.js'

// ── Rendering tests ─────────────────────────────────────────────────

test.describe('Admin page', () => {
  test('renders User Management section', async ({ page }) => {
    await navigateTo(page, '/admin')

    const userHeader = page.locator('h2.section-header', { hasText: 'USER MANAGEMENT' })
    await expect(userHeader).toBeVisible()
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

// ── Provider pool rendering tests ───────────────────────────────────

const mockRegistry = {
  providers: [
    { id: 'kiro', display_name: 'Kiro', category: 'device_code', supports_pool: true },
    { id: 'anthropic', display_name: 'Anthropic', category: 'oauth_relay', supports_pool: true },
    { id: 'openai_codex', display_name: 'OpenAI Codex', category: 'oauth_relay', supports_pool: true },
    { id: 'copilot', display_name: 'Copilot', category: 'device_code', supports_pool: true },
  ],
}

test.describe('Admin page — provider pool', () => {
  test.beforeEach(async ({ page }) => {
    // Admin page fetches provider registry to populate pool dropdown
    await page.route('**/api/providers/registry', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockRegistry),
      })
    })
  })

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
