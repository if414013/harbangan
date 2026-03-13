import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Status } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

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

async function mockProvidersStatus(page: Page) {
  await page.route('**/_ui/api/providers/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        providers: {
          anthropic: { connected: false },
          openai: { connected: false },
        },
      }),
    })
  )
}

// --- Tests ---

test.describe('CopilotSetup component on Profile page', () => {
  test.beforeEach(async ({ page }) => {
    await mockSession(page)
    await mockKiroStatus(page)
    await mockApiKeys(page)
    await mockProvidersStatus(page)
  })

  test.describe('Connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_CONNECTED)
    })

    test('shows card title "github copilot"', async ({ page }) => {
      await navigateTo(page, '/profile')
      const title = page.locator('span.card-title', { hasText: 'github copilot' })
      await expect(title).toBeVisible()
    })

    test('shows CONNECTED badge', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'GITHUB COPILOT' }).locator('~ div')
      const card = section.first()
      await expect(card.locator(Status.ok)).toBeVisible()
      await expect(card.locator(Status.ok)).toContainText('CONNECTED')
    })

    test('shows github username', async ({ page }) => {
      await navigateTo(page, '/profile')
      const username = page.locator('.copilot-username')
      await expect(username).toBeVisible()
      await expect(username).toContainText('octocat')
    })

    test('shows copilot plan', async ({ page }) => {
      await navigateTo(page, '/profile')
      const plan = page.locator('.copilot-plan')
      await expect(plan).toBeVisible()
      await expect(plan).toContainText('business')
    })

    test('shows "$ reconnect" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const btn = page.locator('button.btn-save', { hasText: '$ reconnect' })
      await expect(btn).toBeVisible()
    })

    test('shows "disconnect" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const btn = page.locator('button.device-code-cancel', { hasText: 'disconnect' })
      await expect(btn).toBeVisible()
    })
  })

  test.describe('Expired state', () => {
    test.beforeEach(async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_EXPIRED)
    })

    test('shows EXPIRED badge', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'GITHUB COPILOT' }).locator('~ div')
      const card = section.first()
      await expect(card.locator(Status.warn)).toBeVisible()
      await expect(card.locator(Status.warn)).toContainText('EXPIRED')
    })

    test('still shows github username when expired', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('.copilot-username')).toContainText('octocat')
    })

    test('shows "$ reconnect" button when expired', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('button.btn-save', { hasText: '$ reconnect' })).toBeVisible()
    })

    test('shows "disconnect" button when expired', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('button.device-code-cancel', { hasText: 'disconnect' })).toBeVisible()
    })
  })

  test.describe('Not connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_DISCONNECTED)
    })

    test('shows NOT CONNECTED badge', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'GITHUB COPILOT' }).locator('~ div')
      const card = section.first()
      await expect(card.locator(Status.err)).toBeVisible()
      await expect(card.locator(Status.err)).toContainText('NOT CONNECTED')
    })

    test('does not show github username', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('.copilot-username')).not.toBeAttached()
    })

    test('does not show copilot plan', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('.copilot-plan')).not.toBeAttached()
    })

    test('shows "$ connect github" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('button.btn-save', { hasText: '$ connect github' })).toBeVisible()
    })

    test('does not show disconnect button', async ({ page }) => {
      await navigateTo(page, '/profile')
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

      await page.goto('./profile')
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

      await navigateTo(page, '/profile')
      await page.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
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

      await navigateTo(page, '/profile')
      await page.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
      await expectToastMessage(page, 'Failed to disconnect', 'error')
    })
  })

  test.describe('Connect button behavior', () => {
    test('connect button navigates to copilot connect endpoint', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_DISCONNECTED)
      await navigateTo(page, '/profile')

      // Intercept navigation to verify the connect URL
      const [request] = await Promise.all([
        page.waitForRequest(req => req.url().includes('/_ui/api/copilot/connect')),
        page.locator('button.btn-save', { hasText: '$ connect github' }).click(),
      ])
      expect(request.url()).toContain('/_ui/api/copilot/connect')
    })
  })

  test.describe('Query parameter handling', () => {
    test('shows success toast when ?copilot=connected is in URL', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_CONNECTED)
      await page.goto('./profile?copilot=connected')
      await expectToastMessage(page, 'GitHub Copilot connected', 'success')
    })

    test('shows error toast when ?copilot=error is in URL', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_DISCONNECTED)
      await page.goto('./profile?copilot=error&message=Token+expired')
      await expectToastMessage(page, 'Token expired', 'error')
    })

    test('shows default error message when ?copilot=error without message', async ({ page }) => {
      await mockCopilotStatus(page, COPILOT_DISCONNECTED)
      await page.goto('./profile?copilot=error')
      await expectToastMessage(page, 'Connection failed', 'error')
    })
  })
})
