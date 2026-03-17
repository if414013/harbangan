import { test, expect } from '@playwright/test'
import { Table, Status, Toast } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

// --- Mock data ---

const mockPoolAccounts = [
  {
    id: 'pool-1',
    provider_id: 'anthropic',
    account_label: 'team-primary',
    key_prefix: 'sk-ant-***',
    base_url: null,
    enabled: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'pool-2',
    provider_id: 'openai_codex',
    account_label: 'codex-backup',
    key_prefix: 'sk-***',
    base_url: 'https://custom.openai.example.com',
    enabled: false,
    created_at: '2026-01-02T00:00:00Z',
    updated_at: '2026-01-02T00:00:00Z',
  },
]

const mockUserAccounts = [
  {
    provider_id: 'anthropic',
    account_label: 'personal-key',
    email: 'user@example.com',
    base_url: null,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    provider_id: 'anthropic',
    account_label: 'work-key',
    email: null,
    base_url: null,
    created_at: '2026-01-02T00:00:00Z',
  },
]

const mockRateLimits = [
  {
    provider_id: 'anthropic',
    account_label: 'personal-key',
    requests_remaining: 42,
    tokens_remaining: 100000,
    limited_until: null,
    updated_at: '2026-01-01T12:00:00Z',
  },
  {
    provider_id: 'anthropic',
    account_label: 'work-key',
    requests_remaining: null,
    tokens_remaining: null,
    limited_until: '2026-01-01T13:00:00Z',
    updated_at: '2026-01-01T12:00:00Z',
  },
]

// --- Admin Page: Provider Pool Section ---

test.describe('Admin page — Provider Pool', () => {
  test('renders PROVIDER POOL section header', async ({ page }) => {
    await navigateTo(page, '/admin')

    const header = page.locator('h2.section-header', { hasText: 'PROVIDER POOL' })
    await expect(header).toBeVisible()
  })

  test('add pool account form is visible with provider dropdown, label, and API key inputs', async ({ page }) => {
    await navigateTo(page, '/admin')

    // Provider dropdown
    const providerSelect = page.locator('select.config-input')
    await expect(providerSelect).toBeVisible()

    // Label input
    const labelInput = page.locator('input.config-input[placeholder="account label"]')
    await expect(labelInput).toBeVisible()

    // API key input
    const apiKeyInput = page.locator('input.config-input[placeholder="API key"]')
    await expect(apiKeyInput).toBeVisible()

    // Base URL input (optional)
    const baseUrlInput = page.locator('input.config-input[placeholder="base URL (optional)"]')
    await expect(baseUrlInput).toBeVisible()

    // Submit button
    const addBtn = page.locator('button.btn-save', { hasText: 'Add Pool Account' })
    await expect(addBtn).toBeVisible()
  })

  test('add pool account via form and verify it appears in the table', async ({ page }) => {
    // Start with empty pool, then return the new account after add
    let poolState: typeof mockPoolAccounts = []

    await page.route('**/api/admin/pool', async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ accounts: poolState }),
        })
      } else if (route.request().method() === 'POST') {
        const newAccount = {
          id: 'pool-new',
          provider_id: 'anthropic',
          account_label: 'new-account',
          key_prefix: 'sk-ant-***',
          base_url: null,
          enabled: true,
          created_at: '2026-03-17T00:00:00Z',
          updated_at: '2026-03-17T00:00:00Z',
        }
        poolState = [newAccount]
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(newAccount),
        })
      } else {
        await route.continue()
      }
    })

    await navigateTo(page, '/admin')

    // Empty state should show initially
    const emptyState = page.locator('.empty-state', { hasText: 'No pool accounts configured' })
    await expect(emptyState).toBeVisible()

    // Fill the form
    await page.locator('input.config-input[placeholder="account label"]').fill('new-account')
    await page.locator('input.config-input[placeholder="API key"]').fill('sk-ant-test-key-123')

    // Submit
    await page.locator('button.btn-save', { hasText: 'Add Pool Account' }).click()

    // Verify success toast
    await expectToastMessage(page, 'added', 'success')

    // Verify the new account appears in the data table
    const table = page.locator(Table.dataTable)
    await expect(table).toBeVisible({ timeout: 5_000 })
    await expect(table.locator('td', { hasText: 'new-account' })).toBeVisible()
    await expect(table.locator('td', { hasText: 'anthropic' })).toBeVisible()
  })

  test('toggle pool account enable/disable and verify state changes', async ({ page }) => {
    await page.route('**/api/admin/pool', async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ accounts: mockPoolAccounts }),
        })
      } else {
        await route.continue()
      }
    })

    await page.route('**/api/admin/pool/*/toggle', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: '{}',
      })
    })

    await navigateTo(page, '/admin')

    const table = page.locator(Table.dataTable)
    await expect(table).toBeVisible({ timeout: 5_000 })

    // First account (pool-1) is enabled — toggle button should show "on"
    const firstRow = table.locator('tbody tr').first()
    const toggleBtn = firstRow.locator('.role-badge')
    await expect(toggleBtn).toHaveText('on')

    // Click to disable
    await toggleBtn.click()
    await expect(toggleBtn).toHaveText('off')
  })

  test('delete pool account and verify removal from table', async ({ page }) => {
    await page.route('**/api/admin/pool', async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ accounts: [mockPoolAccounts[0]] }),
        })
      } else {
        await route.continue()
      }
    })

    await page.route('**/api/admin/pool/*', async (route) => {
      if (route.request().method() === 'DELETE') {
        await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
      } else {
        await route.continue()
      }
    })

    await navigateTo(page, '/admin')

    const table = page.locator(Table.dataTable)
    await expect(table).toBeVisible({ timeout: 5_000 })
    await expect(table.locator('td', { hasText: 'team-primary' })).toBeVisible()

    // Accept the confirm dialog
    page.on('dialog', (dialog) => dialog.accept())

    // Click delete
    const deleteBtn = table.locator('.device-code-cancel', { hasText: 'delete' })
    await deleteBtn.click()

    // Verify toast and removal
    await expectToastMessage(page, 'deleted', 'success')
    await expect(table.locator('td', { hasText: 'team-primary' })).not.toBeVisible()
  })

  test('pool table shows correct columns', async ({ page }) => {
    await page.route('**/api/admin/pool', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ accounts: mockPoolAccounts }),
      })
    })

    await navigateTo(page, '/admin')

    const table = page.locator(Table.dataTable)
    await expect(table).toBeVisible({ timeout: 5_000 })

    await expect(table.locator('th', { hasText: 'status' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'provider' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'label' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'key prefix' })).toBeVisible()
    await expect(table.locator('th', { hasText: 'base url' })).toBeVisible()
  })

  test('empty pool shows empty state message', async ({ page }) => {
    await page.route('**/api/admin/pool', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ accounts: [] }),
      })
    })

    await navigateTo(page, '/admin')

    const emptyState = page.locator('.empty-state', { hasText: 'No pool accounts configured' })
    await expect(emptyState).toBeVisible({ timeout: 5_000 })
  })
})

// --- Providers Page: Multi-Account UI ---

test.describe('Providers page — Multi-Account', () => {
  /** Mock all provider-related endpoints for the Providers page. */
  async function setupProviderMocks(
    page: import('@playwright/test').Page,
    overrides: {
      accounts?: typeof mockUserAccounts
      rateLimits?: typeof mockRateLimits
      connected?: boolean
    } = {},
  ) {
    const accounts = overrides.accounts ?? mockUserAccounts
    const rateLimits = overrides.rateLimits ?? mockRateLimits
    const connected = overrides.connected ?? true

    await page.route('**/api/providers/status', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          providers: {
            anthropic: { connected, email: 'user@example.com' },
            openai_codex: { connected: false, email: null },
          },
        }),
      })
    })

    await page.route('**/api/providers/*/accounts', async (route) => {
      if (route.request().method() === 'GET') {
        const url = route.request().url()
        // Match the provider from URL path
        const providerMatch = url.match(/providers\/([^/]+)\/accounts/)
        const provider = providerMatch?.[1] ?? ''
        const filtered = accounts.filter((a) => a.provider_id === provider)
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ accounts: filtered }),
        })
      } else if (route.request().method() === 'DELETE') {
        await route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
      } else {
        await route.continue()
      }
    })

    await page.route('**/api/providers/rate-limits', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ accounts: rateLimits }),
      })
    })

    await page.route('**/api/registry/models', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      })
    })
  }

  test('provider card renders account list for connected providers', async ({ page }) => {
    await setupProviderMocks(page)
    await navigateTo(page, '/providers')

    // Expand the Anthropic tree node
    const anthropicToggle = page.locator('.tree-node-toggle', { hasText: 'Anthropic' })
    await expect(anthropicToggle).toBeVisible({ timeout: 10_000 })
    await anthropicToggle.click()

    // Provider card should be visible with connected status
    const providerCard = page.locator('.provider-card').first()
    await expect(providerCard).toBeVisible()
    await expect(providerCard.locator('.tag-ok', { hasText: 'CONNECTED' })).toBeVisible()

    // Account list should render with account labels
    const accountList = providerCard.locator('.account-list')
    await expect(accountList).toBeVisible()
    await expect(accountList.locator('.account-label', { hasText: 'personal-key' })).toBeVisible()
    await expect(accountList.locator('.account-label', { hasText: 'work-key' })).toBeVisible()
  })

  test('connect another button is present for connected providers', async ({ page }) => {
    await setupProviderMocks(page)
    await navigateTo(page, '/providers')

    // Expand the Anthropic tree node
    await page.locator('.tree-node-toggle', { hasText: 'Anthropic' }).click()

    const providerCard = page.locator('.provider-card').first()
    await expect(providerCard).toBeVisible()

    const connectAnotherBtn = providerCard.locator('.btn-save', { hasText: '$ connect another' })
    await expect(connectAnotherBtn).toBeVisible()
  })

  test('rate limit status indicators are visible', async ({ page }) => {
    await setupProviderMocks(page)
    await navigateTo(page, '/providers')

    // Expand the Anthropic tree node
    await page.locator('.tree-node-toggle', { hasText: 'Anthropic' }).click()

    const providerCard = page.locator('.provider-card').first()
    await expect(providerCard).toBeVisible()

    // personal-key has requests_remaining = 42
    const personalRow = providerCard.locator('.account-row').filter({ hasText: 'personal-key' })
    await expect(personalRow.locator('.account-rate')).toContainText('42 req left')

    // work-key is rate limited
    const workRow = providerCard.locator('.account-row').filter({ hasText: 'work-key' })
    await expect(workRow.locator('.tag-warn', { hasText: 'RATE LIMITED' })).toBeVisible()
  })

  test('delete account from provider card shows confirm and removes it', async ({ page }) => {
    await setupProviderMocks(page)
    await navigateTo(page, '/providers')

    // Expand the Anthropic tree node
    await page.locator('.tree-node-toggle', { hasText: 'Anthropic' }).click()

    const providerCard = page.locator('.provider-card').first()
    await expect(providerCard).toBeVisible()

    const personalRow = providerCard.locator('.account-row').filter({ hasText: 'personal-key' })
    await expect(personalRow).toBeVisible()

    // Accept the confirm dialog
    page.on('dialog', (dialog) => {
      expect(dialog.message()).toContain('Remove account')
      dialog.accept()
    })

    // Click remove
    await personalRow.locator('.device-code-cancel', { hasText: 'remove' }).click()

    // Verify success toast
    await expectToastMessage(page, 'removed', 'success')
  })

  test('not connected provider shows connect button instead of connect another', async ({ page }) => {
    await setupProviderMocks(page, { connected: false, accounts: [], rateLimits: [] })
    await navigateTo(page, '/providers')

    // Expand the Anthropic tree node
    await page.locator('.tree-node-toggle', { hasText: 'Anthropic' }).click()

    const providerCard = page.locator('.provider-card').first()
    await expect(providerCard).toBeVisible()
    await expect(providerCard.locator('.tag-err', { hasText: 'NOT CONNECTED' })).toBeVisible()

    // Should show "$ connect" not "$ connect another"
    await expect(providerCard.locator('.btn-save', { hasText: '$ connect' })).toBeVisible()
    await expect(providerCard.locator('.btn-save', { hasText: '$ connect another' })).not.toBeVisible()
  })

  test('disconnect all button is visible for connected providers', async ({ page }) => {
    await setupProviderMocks(page)
    await navigateTo(page, '/providers')

    // Expand the Anthropic tree node
    await page.locator('.tree-node-toggle', { hasText: 'Anthropic' }).click()

    const providerCard = page.locator('.provider-card').first()
    await expect(providerCard).toBeVisible()

    const disconnectBtn = providerCard.locator('.device-code-cancel', { hasText: '$ disconnect all' })
    await expect(disconnectBtn).toBeVisible()
  })
})
