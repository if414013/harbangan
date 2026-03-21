import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Status } from '../../helpers/selectors.js'
import { navigateTo, waitForPageLoad, expectToastMessage } from '../../helpers/navigation.js'

// --- Types ---

interface ProviderStatus {
  connected: boolean
  email?: string
}

interface ProvidersStatusData {
  providers: Record<string, ProviderStatus>
}

// --- Mock data ---

const MOCK_USER = {
  id: '00000000-0000-0000-0000-000000000001',
  email: 'test@example.com',
  name: 'Test User',
  picture_url: null,
  role: 'user',
  last_login: null,
  created_at: '2026-01-01T00:00:00Z',
}

const PROVIDERS_MIXED: ProvidersStatusData = {
  providers: {
    anthropic: { connected: true, email: 'user@anthropic.com' },
    openai_codex: { connected: false },
  },
}

const PROVIDERS_ALL_DISCONNECTED: ProvidersStatusData = {
  providers: {
    anthropic: { connected: false },
    openai_codex: { connected: false },
  },
}

const PROVIDERS_ALL_CONNECTED: ProvidersStatusData = {
  providers: {
    anthropic: { connected: true, email: 'a@anthropic.com' },
    openai_codex: { connected: true, email: 'o@openai.com' },
  },
}

// --- Helpers ---

async function mockSession(page: Page) {
  await page.route('**/_ui/api/auth/me', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(MOCK_USER),
    })
  )
  await page.route('**/_ui/api/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ setup_complete: true }),
    })
  )
}

async function mockProvidersStatus(page: Page, data: ProvidersStatusData = PROVIDERS_MIXED) {
  await page.route('**/_ui/api/providers/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(data),
    })
  )
}

/** Mock Providers page dependencies (excluding providers/status which is mocked separately) */
async function mockProvidersPageDeps(page: Page) {
  await page.route('**/_ui/api/kiro/status', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ has_token: false, expired: false }) })
  )
  await page.route('**/_ui/api/copilot/status', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ connected: false, github_username: null, copilot_plan: null, expired: false }) })
  )
  await page.route('**/_ui/api/models/registry', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ models: [] }) })
  )
  await page.route('**/_ui/api/providers/anthropic/accounts', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ accounts: [] }) })
  )
  await page.route('**/_ui/api/providers/openai_codex/accounts', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ accounts: [] }) })
  )
  await page.route('**/_ui/api/providers/rate-limits', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ accounts: [] }) })
  )
}

/** Navigate to Providers page connections tab */
async function navigateToConnections(page: Page) {
  await navigateTo(page, '/providers')
  await page.locator('[role="tab"]', { hasText: 'connections' }).click()
  await page.locator('h2.section-header', { hasText: 'Multi-Account Providers' }).waitFor()
  await waitForPageLoad(page)
}

// --- Tests ---

test.describe('Provider OAuth on Providers page', () => {
  test.beforeEach(async ({ page }) => {
    await mockSession(page)
    await mockProvidersPageDeps(page)
  })

  test.describe('Multi-Account Providers section structure', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('renders Multi-Account Providers section header', async ({ page }) => {
      await navigateToConnections(page)
      const header = page.locator('h2.section-header', { hasText: 'Multi-Account Providers' })
      await expect(header).toBeVisible()
    })

    test('renders 2 provider cards (anthropic, openai_codex)', async ({ page }) => {
      await navigateToConnections(page)
      const cards = page.locator('div.provider-card')
      await expect(cards).toHaveCount(2)
    })

    test('renders providers in order: Anthropic, OpenAI Codex', async ({ page }) => {
      await navigateToConnections(page)
      const titles = page.locator('div.provider-card span.card-title')
      await expect(titles.nth(0)).toContainText('Anthropic')
      await expect(titles.nth(1)).toContainText('OpenAI Codex')
    })

    test('card titles are prefixed with "> "', async ({ page }) => {
      await navigateToConnections(page)
      const title = page.locator('div.provider-card span.card-title').first()
      await expect(title).toContainText('> Anthropic')
    })

    test('does not include kiro in Multi-Account Providers section', async ({ page }) => {
      await navigateToConnections(page)
      const kiroCard = page.locator('div.provider-card').filter({ hasText: 'kiro' })
      await expect(kiroCard).toHaveCount(0)
    })
  })

  test.describe('Connection status indicators', () => {
    test('shows CONNECTED badge for connected provider', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: '> Anthropic' })
      await expect(card.locator(Status.ok)).toBeVisible()
      await expect(card.locator(Status.ok)).toContainText('CONNECTED')
    })

    test('shows NOT CONNECTED badge for disconnected provider', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await expect(card.locator(Status.err)).toBeVisible()
      await expect(card.locator(Status.err)).toContainText('NOT CONNECTED')
    })

    test('both providers show NOT CONNECTED when all disconnected', async ({ page }) => {
      await mockProvidersStatus(page, PROVIDERS_ALL_DISCONNECTED)
      await navigateToConnections(page)
      const badges = page.locator('div.provider-card').locator(Status.err)
      await expect(badges).toHaveCount(2)
    })

    test('both providers show CONNECTED when all connected', async ({ page }) => {
      await mockProvidersStatus(page, PROVIDERS_ALL_CONNECTED)
      await navigateToConnections(page)
      const badges = page.locator('div.provider-card').locator(Status.ok)
      await expect(badges).toHaveCount(2)
    })
  })

  test.describe('Connected provider details', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('shows email for connected provider', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: '> Anthropic' })
      await expect(card.locator('.provider-email')).toContainText('user@anthropic.com')
    })

    test('does not show email for disconnected provider', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await expect(card.locator('.provider-email')).not.toBeAttached()
    })
  })

  test.describe('Action buttons', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('connected provider shows "$ disconnect all" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: '> Anthropic' })
      await expect(card.locator('button.btn-danger', { hasText: '$ disconnect all' })).toBeVisible()
    })

    test('connected provider shows "$ connect another" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: '> Anthropic' })
      await expect(card.locator('button.btn-save', { hasText: '$ connect another' })).toBeVisible()
    })

    test('disconnected provider shows "$ connect" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await expect(card.locator('button.btn-save', { hasText: '$ connect' })).toBeVisible()
    })

    test('disconnected provider does not show disconnect button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await expect(card.locator('button.btn-danger')).not.toBeAttached()
    })
  })

  test.describe('Connect flow — relay modal', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page, PROVIDERS_ALL_DISCONNECTED)
      await page.route('**/_ui/api/providers/openai_codex/connect', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            relay_script_url: 'https://gw.example.com/_ui/api/providers/openai_codex/relay-script?token=abc123',
          }),
        })
      )
    })

    test('clicking "$ connect" opens relay modal', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()
    })

    test('relay modal shows provider name in heading', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal h3')).toContainText('connect openai_codex')
    })

    test('relay modal shows curl command', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      const command = page.locator('.relay-command')
      await expect(command).toBeVisible()
      await expect(command).toContainText('curl -fsSL')
      await expect(command).toContainText('relay-script?token=abc123')
    })

    test('relay modal shows [copy] button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-copy-btn')).toBeVisible()
      await expect(page.locator('.relay-copy-btn')).toContainText('[copy]')
    })

    test('relay modal shows "waiting for authorization..." polling indicator', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.device-code-polling')).toContainText('waiting for authorization...')
    })

    test('relay modal has cancel button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.modal-actions button', { hasText: '$ cancel' })).toBeVisible()
    })

    test('cancel button closes the relay modal', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()
      await page.locator('.modal-actions button', { hasText: '$ cancel' }).click()
      await expect(page.locator('.relay-modal')).not.toBeAttached()
    })

    test('clicking overlay closes the relay modal', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()
      // Click the overlay (outside the modal box)
      await page.locator('.modal-overlay').click({ position: { x: 5, y: 5 } })
      await expect(page.locator('.relay-modal')).not.toBeAttached()
    })
  })

  test.describe('Connect flow — polling detects connection', () => {
    test('shows success toast when provider becomes connected during polling', async ({ page }) => {
      let pollCount = 0
      await page.route('**/_ui/api/providers/status', route => {
        pollCount++
        // First call: all disconnected (initial load + first poll)
        // After 2 polls: openai_codex becomes connected
        const openaiConnected = pollCount > 2
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            providers: {
              anthropic: { connected: false },
              openai_codex: { connected: openaiConnected, email: openaiConnected ? 'o@openai.com' : undefined },
            },
          }),
        })
      })
      await page.route('**/_ui/api/providers/openai_codex/connect', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            relay_script_url: 'https://gw.example.com/_ui/api/providers/openai_codex/relay-script?token=abc123',
          }),
        })
      )

      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: 'OpenAI Codex' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()

      // Wait for polling to detect connection
      await expectToastMessage(page, 'openai_codex connected', 'success')
      // Modal should close after successful connection
      await expect(page.locator('.relay-modal')).not.toBeAttached()
    })
  })

  test.describe('Disconnect flow', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('clicking "$ disconnect all" sends DELETE and shows success toast', async ({ page }) => {
      let capturedMethod = ''
      await page.route('**/_ui/api/providers/anthropic', route => {
        if (route.request().method() === 'DELETE') {
          capturedMethod = route.request().method()
          route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
        } else {
          route.continue()
        }
      })

      await navigateToConnections(page)
      const card = page.locator('div.provider-card').filter({ hasText: '> Anthropic' })
      await card.locator('button.btn-danger', { hasText: '$ disconnect all' }).click()
      await expectToastMessage(page, 'anthropic disconnected', 'success')
      expect(capturedMethod).toBe('DELETE')
    })
  })

  test.describe('Providers page verification', () => {
    test('sidebar has a "providers" nav link', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateTo(page, '/providers')
      const providerLink = page.locator('a.nav-link', { hasText: 'providers' })
      await expect(providerLink).toBeVisible()
    })

    test('providers page renders with tab bar', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateTo(page, '/providers')
      const tabBar = page.locator('[role="tablist"]')
      await expect(tabBar).toBeVisible()
      await expect(page.locator('[role="tab"]', { hasText: 'status' })).toBeVisible()
      await expect(page.locator('[role="tab"]', { hasText: 'connections' })).toBeVisible()
      await expect(page.locator('[role="tab"]', { hasText: 'models' })).toBeVisible()
    })

    test('no API key input exists in the provider cards', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateToConnections(page)
      const providerCards = page.locator('div.provider-card')
      // No password input (old key entry form)
      await expect(providerCards.locator('input[type="password"]')).toHaveCount(0)
      // No "add key" or "replace key" buttons
      await expect(providerCards.locator('button', { hasText: 'add key' })).toHaveCount(0)
      await expect(providerCards.locator('button', { hasText: 'replace key' })).toHaveCount(0)
    })
  })
})
