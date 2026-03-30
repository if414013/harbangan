import { test, expect } from '@playwright/test'
import { navigateTo } from '../../helpers/navigation.js'

const mockRegistry = {
  providers: [
    { id: 'kiro', display_name: 'Kiro', category: 'device_code', supports_pool: true, enabled: true },
    { id: 'anthropic', display_name: 'Anthropic', category: 'oauth_relay', supports_pool: true, enabled: true },
    { id: 'openai_codex', display_name: 'OpenAI Codex', category: 'oauth_relay', supports_pool: true, enabled: true },
    { id: 'copilot', display_name: 'Copilot', category: 'device_code', supports_pool: true, enabled: true },
  ],
}

const mockProviderStatus = {
  providers: {
    kiro: { connected: true },
    anthropic: { connected: true },
    openai_codex: { connected: false },
    copilot: { connected: false },
  },
}

const mockModels = { models: [] }

function setupMocks(page: import('@playwright/test').Page, registryOverride?: typeof mockRegistry) {
  const registry = registryOverride ?? mockRegistry
  return Promise.all([
    page.route('**/api/providers/registry', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(registry) }),
    ),
    page.route('**/api/providers/status', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(mockProviderStatus) }),
    ),
    page.route('**/api/kiro/status', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ has_token: true, expired: false }) }),
    ),
    page.route('**/api/copilot/status', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ connected: false, has_copilot_token: false, expired: true }) }),
    ),
    page.route('**/api/models/registry', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(mockModels) }),
    ),
    page.route('**/api/providers/*/accounts', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ accounts: [] }) }),
    ),
    page.route('**/api/providers/rate-limits', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ accounts: [] }) }),
    ),
    page.route('**/api/models/visibility/defaults', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ defaults: {} }) }),
    ),
  ])
}

test.describe('Provider toggle — admin controls', () => {
  test.beforeEach(async ({ page }) => {
    await setupMocks(page)
  })

  test('admin sees toggle badges on non-Kiro providers', async ({ page }) => {
    await navigateTo(page, '/providers')

    const grid = page.locator('.health-grid')
    await expect(grid).toBeVisible()

    // Anthropic, OpenAI Codex, Copilot should have toggle badges
    for (const name of ['Anthropic', 'OpenAI Codex', 'Copilot']) {
      const card = grid.locator('.health-card', { hasText: name })
      const toggle = card.locator('.role-badge')
      await expect(toggle).toBeVisible()
      await expect(toggle).toHaveText('on')
    }
  })

  test('Kiro card has no toggle badge', async ({ page }) => {
    await navigateTo(page, '/providers')

    const kiroCard = page.locator('.health-card', { hasText: 'Kiro' })
    await expect(kiroCard).toBeVisible()
    await expect(kiroCard.locator('.role-badge')).not.toBeVisible()
  })

  test('clicking toggle disables provider', async ({ page }) => {
    // Mock the PATCH endpoint to succeed
    await page.route('**/api/admin/providers/anthropic', (route) => {
      if (route.request().method() === 'PATCH') {
        return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ success: true }) })
      }
      return route.continue()
    })

    await navigateTo(page, '/providers')

    const anthropicCard = page.locator('.health-card', { hasText: 'Anthropic' })
    const toggle = anthropicCard.locator('.role-badge')
    await expect(toggle).toHaveText('on')

    await toggle.click()

    await expect(toggle).toHaveText('off')
    await expect(anthropicCard.locator('.health-card-status')).toHaveText('Disabled')
  })

  test('clicking toggle re-enables provider', async ({ page }) => {
    // Start with anthropic disabled
    const disabledRegistry = {
      providers: mockRegistry.providers.map((p) =>
        p.id === 'anthropic' ? { ...p, enabled: false } : p,
      ),
    }
    // Re-setup mocks with disabled anthropic
    await page.unrouteAll()
    await setupMocks(page, disabledRegistry)

    await page.route('**/api/admin/providers/anthropic', (route) => {
      if (route.request().method() === 'PATCH') {
        return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ success: true }) })
      }
      return route.continue()
    })

    await navigateTo(page, '/providers')

    const anthropicCard = page.locator('.health-card', { hasText: 'Anthropic' })
    const toggle = anthropicCard.locator('.role-badge')
    await expect(toggle).toHaveText('off')
    await expect(anthropicCard.locator('.health-card-status')).toHaveText('Disabled')

    await toggle.click()

    await expect(toggle).toHaveText('on')
    await expect(anthropicCard.locator('.health-card-status')).toHaveText('Connected')
  })

  test('disabled provider card shows data-connected=false', async ({ page }) => {
    const disabledRegistry = {
      providers: mockRegistry.providers.map((p) =>
        p.id === 'anthropic' ? { ...p, enabled: false } : p,
      ),
    }
    await page.unrouteAll()
    await setupMocks(page, disabledRegistry)

    await navigateTo(page, '/providers')

    const anthropicCard = page.locator('.health-card', { hasText: 'Anthropic' })
    await expect(anthropicCard).toHaveAttribute('data-connected', 'false')
  })

  test('disabled provider hides meta section', async ({ page }) => {
    const disabledRegistry = {
      providers: mockRegistry.providers.map((p) =>
        p.id === 'anthropic' ? { ...p, enabled: false } : p,
      ),
    }
    await page.unrouteAll()
    await setupMocks(page, disabledRegistry)

    await navigateTo(page, '/providers')

    const anthropicCard = page.locator('.health-card', { hasText: 'Anthropic' })
    await expect(anthropicCard.locator('.health-card-meta')).not.toBeVisible()
  })

  test('summary bar reflects enabled provider count', async ({ page }) => {
    const disabledRegistry = {
      providers: mockRegistry.providers.map((p) =>
        p.id === 'anthropic' ? { ...p, enabled: false } : p,
      ),
    }
    await page.unrouteAll()
    await setupMocks(page, disabledRegistry)

    await navigateTo(page, '/providers')

    const summaryBar = page.locator('.summary-bar')
    await expect(summaryBar).toBeVisible()
    // Only kiro is connected+enabled out of 3 enabled providers
    await expect(summaryBar).toContainText('1/3 providers connected')
  })
})
