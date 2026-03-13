import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Status } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

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
    openai: { connected: false },
  },
}

const PROVIDERS_ALL_DISCONNECTED: ProvidersStatusData = {
  providers: {
    anthropic: { connected: false },
    openai: { connected: false },
  },
}

const PROVIDERS_ALL_CONNECTED: ProvidersStatusData = {
  providers: {
    anthropic: { connected: true, email: 'a@anthropic.com' },
    openai: { connected: true, email: 'o@openai.com' },
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

async function mockKiroStatus(page: Page) {
  await page.route('**/_ui/api/kiro/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ has_token: false, expired: false }),
    })
  )
}

async function mockApiKeys(page: Page) {
  await page.route('**/_ui/api/keys', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ keys: [] }),
    })
  )
}

// --- Tests ---

test.describe('Provider OAuth on Profile page', () => {
  test.beforeEach(async ({ page }) => {
    await mockSession(page)
    await mockKiroStatus(page)
    await mockApiKeys(page)
  })

  test.describe('PROVIDERS section structure', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('renders PROVIDERS section header', async ({ page }) => {
      await navigateTo(page, '/profile')
      const header = page.locator('h2.section-header', { hasText: 'PROVIDERS' })
      await expect(header).toBeVisible()
    })

    test('renders 2 provider cards (anthropic, openai — no kiro)', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('.providers-grid')
      const cards = section.locator('div.provider-card')
      await expect(cards).toHaveCount(2)
    })

    test('renders providers in order: anthropic, openai', async ({ page }) => {
      await navigateTo(page, '/profile')
      const titles = page.locator('.providers-grid span.card-title')
      await expect(titles.nth(0)).toContainText('anthropic')
      await expect(titles.nth(1)).toContainText('openai')
    })

    test('card titles are prefixed with "> "', async ({ page }) => {
      await navigateTo(page, '/profile')
      const title = page.locator('.providers-grid span.card-title').first()
      await expect(title).toContainText('> anthropic')
    })

    test('does not include kiro in PROVIDERS section', async ({ page }) => {
      await navigateTo(page, '/profile')
      const kiroCard = page.locator('.providers-grid div.provider-card').filter({ hasText: 'kiro' })
      await expect(kiroCard).toHaveCount(0)
    })
  })

  test.describe('Connection status indicators', () => {
    test('shows CONNECTED badge for connected provider', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> anthropic' })
      await expect(card.locator(Status.ok)).toBeVisible()
      await expect(card.locator(Status.ok)).toContainText('CONNECTED')
    })

    test('shows NOT CONNECTED badge for disconnected provider', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await expect(card.locator(Status.err)).toBeVisible()
      await expect(card.locator(Status.err)).toContainText('NOT CONNECTED')
    })

    test('both providers show NOT CONNECTED when all disconnected', async ({ page }) => {
      await mockProvidersStatus(page, PROVIDERS_ALL_DISCONNECTED)
      await navigateTo(page, '/profile')
      const badges = page.locator('.providers-grid').locator(Status.err)
      await expect(badges).toHaveCount(2)
    })

    test('both providers show CONNECTED when all connected', async ({ page }) => {
      await mockProvidersStatus(page, PROVIDERS_ALL_CONNECTED)
      await navigateTo(page, '/profile')
      const badges = page.locator('.providers-grid').locator(Status.ok)
      await expect(badges).toHaveCount(2)
    })
  })

  test.describe('Connected provider details', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('shows email for connected provider', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> anthropic' })
      await expect(card.locator('.provider-email')).toContainText('user@anthropic.com')
    })

    test('does not show email for disconnected provider', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await expect(card.locator('.provider-email')).not.toBeAttached()
    })
  })

  test.describe('Action buttons', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('connected provider shows "$ disconnect" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> anthropic' })
      await expect(card.locator('button.device-code-cancel', { hasText: '$ disconnect' })).toBeVisible()
    })

    test('disconnected provider shows "$ connect" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await expect(card.locator('button.btn-save', { hasText: '$ connect' })).toBeVisible()
    })

    test('connected provider does not show connect button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> anthropic' })
      await expect(card.locator('button.btn-save', { hasText: '$ connect' })).not.toBeAttached()
    })

    test('disconnected provider does not show disconnect button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await expect(card.locator('button.device-code-cancel', { hasText: '$ disconnect' })).not.toBeAttached()
    })
  })

  test.describe('Connect flow — relay modal', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page, PROVIDERS_ALL_DISCONNECTED)
      await page.route('**/_ui/api/providers/openai/connect', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            relay_script_url: 'https://gw.example.com/_ui/api/providers/openai/relay-script?token=abc123',
          }),
        })
      )
    })

    test('clicking "$ connect" opens relay modal', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()
    })

    test('relay modal shows provider name in heading', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal h3')).toContainText('connect openai')
    })

    test('relay modal shows curl command', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      const command = page.locator('.relay-command')
      await expect(command).toBeVisible()
      await expect(command).toContainText('curl -fsSL')
      await expect(command).toContainText('relay-script?token=abc123')
    })

    test('relay modal shows [copy] button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-copy-btn')).toBeVisible()
      await expect(page.locator('.relay-copy-btn')).toContainText('[copy]')
    })

    test('relay modal shows "waiting for authorization..." polling indicator', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.device-code-polling')).toContainText('waiting for authorization...')
    })

    test('relay modal has cancel button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.modal-actions button', { hasText: '$ cancel' })).toBeVisible()
    })

    test('cancel button closes the relay modal', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()
      await page.locator('.modal-actions button', { hasText: '$ cancel' }).click()
      await expect(page.locator('.relay-modal')).not.toBeAttached()
    })

    test('clicking overlay closes the relay modal', async ({ page }) => {
      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
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
        // After 2 polls: openai becomes connected
        const openaiConnected = pollCount > 2
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            providers: {
              anthropic: { connected: false },
              openai: { connected: openaiConnected, email: openaiConnected ? 'o@openai.com' : undefined },
            },
          }),
        })
      })
      await page.route('**/_ui/api/providers/openai/connect', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            relay_script_url: 'https://gw.example.com/_ui/api/providers/openai/relay-script?token=abc123',
          }),
        })
      )

      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> openai' })
      await card.locator('button.btn-save', { hasText: '$ connect' }).click()
      await expect(page.locator('.relay-modal')).toBeVisible()

      // Wait for polling to detect connection
      await expectToastMessage(page, 'openai connected', 'success')
      // Modal should close after successful connection
      await expect(page.locator('.relay-modal')).not.toBeAttached()
    })
  })

  test.describe('Disconnect flow', () => {
    test.beforeEach(async ({ page }) => {
      await mockProvidersStatus(page)
    })

    test('clicking "$ disconnect" sends DELETE and shows success toast', async ({ page }) => {
      let capturedMethod = ''
      await page.route('**/_ui/api/providers/anthropic', route => {
        if (route.request().method() === 'DELETE') {
          capturedMethod = route.request().method()
          route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
        } else {
          route.continue()
        }
      })

      await navigateTo(page, '/profile')
      const card = page.locator('div.provider-card').filter({ hasText: '> anthropic' })
      await card.locator('button.device-code-cancel', { hasText: '$ disconnect' }).click()
      await expectToastMessage(page, 'anthropic disconnected', 'success')
      expect(capturedMethod).toBe('DELETE')
    })
  })

  test.describe('Removal verification', () => {
    test('/providers route does not exist (no nav link)', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateTo(page, '/profile')
      // Sidebar should not have a "providers" nav link
      const providerLink = page.locator('a.nav-link', { hasText: 'providers' })
      await expect(providerLink).toHaveCount(0)
    })

    test('/providers route shows fallback (not the old providers page)', async ({ page }) => {
      await mockProvidersStatus(page)
      // Navigate directly to /providers — should not render old providers page
      await page.goto('./providers')
      // The old page had a "provider keys" section header — it should not exist
      const oldHeader = page.locator('div.section-header', { hasText: 'provider keys' })
      await expect(oldHeader).not.toBeAttached()
    })

    test('no API key input exists in the PROVIDERS section', async ({ page }) => {
      await mockProvidersStatus(page)
      await navigateTo(page, '/profile')
      const section = page.locator('.providers-grid')
      // No password input (old key entry form)
      await expect(section.locator('input[type="password"]')).toHaveCount(0)
      // No "add key" or "replace key" buttons
      await expect(section.locator('button', { hasText: 'add key' })).toHaveCount(0)
      await expect(section.locator('button', { hasText: 'replace key' })).toHaveCount(0)
    })
  })
})
