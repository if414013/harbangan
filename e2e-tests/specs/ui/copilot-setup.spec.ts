import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Status } from '../../helpers/selectors.js'
import { navigateTo, waitForPageLoad, expectToastMessage } from '../../helpers/navigation.js'

// --- Types ---

interface CopilotStatusData {
  connected: boolean
  github_username: string | null
  copilot_plan: string | null
  expired: boolean
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

const COPILOT_CONNECTED: CopilotStatusData = {
  connected: true,
  github_username: 'octocat',
  copilot_plan: 'business',
  expired: false,
}

const COPILOT_EXPIRED: CopilotStatusData = {
  connected: true,
  github_username: 'octocat',
  copilot_plan: 'business',
  expired: true,
}

const COPILOT_DISCONNECTED: CopilotStatusData = {
  connected: false,
  github_username: null,
  copilot_plan: null,
  expired: false,
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

async function mockCopilotStatus(page: Page, data: CopilotStatusData = COPILOT_CONNECTED) {
  await page.route('**/_ui/api/copilot/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(data),
    })
  )
}

/** Mock Providers page dependencies (excluding copilot/status which is mocked separately) */
async function mockProvidersPageDeps(page: Page) {
  await page.route('**/_ui/api/kiro/status', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ has_token: false, expired: false }) })
  )
  await page.route('**/_ui/api/providers/status', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ providers: {} }) })
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
  await page.locator('h2.section-header', { hasText: 'Device Code Providers' }).waitFor()
  await waitForPageLoad(page)
}

// --- Tests ---

test.describe('CopilotSetup component on Providers page', () => {
  test.beforeEach(async ({ page }) => {
    await mockSession(page)
    await mockProvidersPageDeps(page)
  })

  test.describe('Connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_CONNECTED)
    })

    test('shows card title "GitHub Copilot"', async ({ page }) => {
      await navigateToConnections(page)
      const title = page.locator('span.card-title', { hasText: 'GitHub Copilot' })
      await expect(title).toBeVisible()
    })

    test('shows CONNECTED badge', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await expect(card.locator(Status.ok)).toBeVisible()
      await expect(card.locator(Status.ok)).toContainText('CONNECTED')
    })

    test('shows github username', async ({ page }) => {
      await navigateToConnections(page)
      const username = page.locator('.copilot-username')
      await expect(username).toBeVisible()
      await expect(username).toContainText('octocat')
    })

    test('shows copilot plan', async ({ page }) => {
      await navigateToConnections(page)
      const plan = page.locator('.copilot-plan')
      await expect(plan).toBeVisible()
      await expect(plan).toContainText('business')
    })

    test('shows "$ reconnect" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await expect(card.locator('button.btn-save', { hasText: '$ reconnect' })).toBeVisible()
    })

    test('shows "disconnect" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await expect(card.locator('button.device-code-cancel', { hasText: 'disconnect' })).toBeVisible()
    })
  })

  test.describe('Expired state', () => {
    test.beforeEach(async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_EXPIRED)
    })

    test('shows EXPIRED badge', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await expect(card.locator(Status.warn)).toBeVisible()
      await expect(card.locator(Status.warn)).toContainText('EXPIRED')
    })

    test('still shows github username when expired', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('.copilot-username')).toContainText('octocat')
    })

    test('shows "$ reconnect" button when expired', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('button.btn-save', { hasText: '$ reconnect' })).toBeVisible()
    })

    test('shows "disconnect" button when expired', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('button.device-code-cancel', { hasText: 'disconnect' })).toBeVisible()
    })
  })

  test.describe('Not connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_DISCONNECTED)
    })

    test('shows NOT CONNECTED badge', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await expect(card.locator(Status.err)).toBeVisible()
      await expect(card.locator(Status.err)).toContainText('NOT CONNECTED')
    })

    test('does not show github username', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('.copilot-username')).not.toBeAttached()
    })

    test('does not show copilot plan', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('.copilot-plan')).not.toBeAttached()
    })

    test('shows "$ connect github" button', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('button.btn-save', { hasText: '$ connect github' })).toBeVisible()
    })

    test('does not show disconnect button', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('button.device-code-cancel', { hasText: 'disconnect' })).not.toBeAttached()
    })
  })

  test.describe('Loading state', () => {
    test('shows skeleton loader while fetching status', async ({ page }) => {
      // Delay the copilot status response to observe loading state
      await page.route('**/_ui/api/copilot/status', async route => {
        await new Promise(r => setTimeout(r, 2000))
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(COPILOT_CONNECTED),
        })
      })

      await page.goto('./providers')
      await page.locator('[role="tab"]', { hasText: 'connections' }).click()
      const skeleton = page.locator('[role="status"][aria-label="Loading Copilot status"]')
      await expect(skeleton).toBeVisible()
    })
  })

  test.describe('Disconnect flow', () => {
    test('clicking disconnect sends DELETE and shows success toast', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_CONNECTED)

      let capturedMethod = ''
      await page.route('**/_ui/api/copilot/disconnect', route => {
        if (route.request().method() === 'DELETE') {
          capturedMethod = route.request().method()
          route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
        } else {
          route.continue()
        }
      })

      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await card.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
      await expectToastMessage(page, 'GitHub Copilot disconnected', 'success')
      expect(capturedMethod).toBe('DELETE')
    })

    test('shows error toast when disconnect fails', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_CONNECTED)

      await page.route('**/_ui/api/copilot/disconnect', route =>
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Internal server error' }),
        })
      )

      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'GitHub Copilot' })
      await card.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
      await expectToastMessage(page, 'Failed to disconnect', 'error')
    })
  })

  test.describe('Connect button behavior', () => {
    test('clicking connect calls POST device-code endpoint', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_DISCONNECTED)

      let capturedMethod = ''
      await page.route('**/_ui/api/copilot/device-code', route => {
        capturedMethod = route.request().method()
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            device_code: 'test-code',
            user_code: 'TEST-1234',
            verification_uri: 'https://github.com/login/device',
            expires_in: 600,
            interval: 5,
          }),
        })
      })
      await page.route('**/_ui/api/copilot/device-poll*', route =>
        route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ status: 'pending' }) })
      )

      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect github' }).click()
      expect(capturedMethod).toBe('POST')
    })
  })
})
