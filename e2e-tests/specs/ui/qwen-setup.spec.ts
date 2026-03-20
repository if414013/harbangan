import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Status } from '../../helpers/selectors.js'
import { navigateTo, waitForPageLoad, expectToastMessage } from '../../helpers/navigation.js'

// --- Types ---

interface QwenStatusData {
  connected: boolean
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

const QWEN_CONNECTED: QwenStatusData = {
  connected: true,
  expired: false,
}

const QWEN_DISCONNECTED: QwenStatusData = {
  connected: false,
  expired: false,
}

const MOCK_DEVICE_CODE = {
  device_code: 'qwen-device-abc123',
  user_code: 'QWEN-1234',
  verification_uri: 'https://chat.qwen.ai/device',
  verification_uri_complete: 'https://chat.qwen.ai/device?user_code=QWEN-1234',
  expires_in: 600,
  interval: 5,
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

async function mockQwenStatus(page: Page, data: QwenStatusData = QWEN_CONNECTED) {
  await page.route('**/_ui/api/providers/qwen/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(data),
    })
  )
}

/** Mock Providers page dependencies (excluding qwen/status which is mocked separately) */
async function mockProvidersPageDeps(page: Page) {
  await page.route('**/_ui/api/kiro/status', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ has_token: false, expired: false }) })
  )
  await page.route('**/_ui/api/copilot/status', route =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ connected: false, github_username: null, copilot_plan: null, expired: false }) })
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

test.describe('QwenSetup component on Providers page', () => {
  test.beforeEach(async ({ page }) => {
    await mockSession(page)
    await mockProvidersPageDeps(page)
  })

  // --- Section structure ---

  test.describe('Section structure', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
    })

    test('renders under Device Code Providers section', async ({ page }) => {
      await navigateToConnections(page)
      const header = page.locator('h2.section-header', { hasText: 'Device Code Providers' })
      await expect(header).toBeVisible()
    })

    test('renders card with title "Qwen Coder"', async ({ page }) => {
      await navigateToConnections(page)
      const title = page.locator('span.card-title', { hasText: 'Qwen Coder' })
      await expect(title).toBeVisible()
    })

    test('card has proper structure with header', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await expect(card).toBeVisible()
      await expect(card.locator('.card-header')).toBeVisible()
    })
  })

  // --- Connected state ---

  test.describe('Connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_CONNECTED)
    })

    test('shows CONNECTED badge', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await expect(card.locator(Status.ok)).toBeVisible()
      await expect(card.locator(Status.ok)).toContainText('CONNECTED')
    })

    test('shows "$ reconnect" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await expect(card.locator('button.btn-save', { hasText: '$ reconnect' })).toBeVisible()
    })

    test('shows "disconnect" button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await expect(card.locator('button.device-code-cancel', { hasText: 'disconnect' })).toBeVisible()
    })

    test('does not show "$ connect qwen" button', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('button.btn-save', { hasText: '$ connect qwen' })).not.toBeAttached()
    })
  })

  // --- Not connected state ---

  test.describe('Not connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
    })

    test('shows NOT CONNECTED badge', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await expect(card.locator(Status.err)).toBeVisible()
      await expect(card.locator(Status.err)).toContainText('NOT CONNECTED')
    })

    test('shows "$ connect qwen" button', async ({ page }) => {
      await navigateToConnections(page)
      await expect(page.locator('button.btn-save', { hasText: '$ connect qwen' })).toBeVisible()
    })

    test('does not show disconnect button', async ({ page }) => {
      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await expect(card.locator('button.device-code-cancel', { hasText: 'disconnect' })).not.toBeAttached()
    })
  })

  // --- Device flow initiation ---

  test.describe('Device flow initiation', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
      await page.route('**/_ui/api/providers/qwen/device-code', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(MOCK_DEVICE_CODE),
        })
      )
      // Mock poll endpoint to keep returning pending
      await page.route('**/_ui/api/providers/qwen/device-poll*', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'pending' }),
        })
      )
    })

    test('clicking connect calls POST device-code endpoint', async ({ page }) => {
      let capturedMethod = ''
      await page.route('**/_ui/api/providers/qwen/device-code', route => {
        capturedMethod = route.request().method()
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(MOCK_DEVICE_CODE),
        })
      })

      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      expect(capturedMethod).toBe('POST')
    })

    test('shows verification URL as clickable link', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      const link = page.locator('a.device-code-link')
      await expect(link).toBeVisible()
      await expect(link).toHaveAttribute('href', MOCK_DEVICE_CODE.verification_uri_complete)
      await expect(link).toHaveAttribute('target', '_blank')
    })

    test('shows user code', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-value')).toContainText('QWEN-1234')
    })

    test('shows copy button', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-copy')).toContainText('[click to copy]')
    })

    test('shows polling indicator', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-polling')).toContainText('polling...')
    })

    test('shows cancel button', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('button.device-code-cancel', { hasText: '$ cancel' })).toBeVisible()
    })

    test('cancel closes device flow and returns to card', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-wrap')).toBeVisible()
      await page.locator('button.device-code-cancel', { hasText: '$ cancel' }).click()
      await expect(page.locator('.device-code-wrap')).not.toBeAttached()
      // Card should be back
      await expect(page.locator('span.card-title', { hasText: 'Qwen Coder' })).toBeVisible()
    })

    test('shows verification URI text', async ({ page }) => {
      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-uri')).toContainText('https://chat.qwen.ai/device')
    })
  })

  // --- Device flow polling states ---

  test.describe('Device flow polling states', () => {
    // DeviceCodeDisplay hardcodes a 5s poll interval, so toasts appear after ~5-10s.
    // Use a longer assertion timeout than the default 5s in expectToastMessage.
    const POLL_TOAST_TIMEOUT = 15_000

    test('successful auth shows success toast and updates status', async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
      await page.route('**/_ui/api/providers/qwen/device-code', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(MOCK_DEVICE_CODE),
        })
      )

      let pollCount = 0
      await page.route('**/_ui/api/providers/qwen/device-poll*', route => {
        pollCount++
        const status = pollCount > 1 ? 'success' : 'pending'
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status }),
        })
      })

      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      const toast = page.locator('div.toast.toast-success').filter({ hasText: 'Qwen Coder connected successfully' })
      await expect(toast).toBeVisible({ timeout: POLL_TOAST_TIMEOUT })
    })

    test('expired code shows error toast', async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
      await page.route('**/_ui/api/providers/qwen/device-code', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(MOCK_DEVICE_CODE),
        })
      )

      await page.route('**/_ui/api/providers/qwen/device-poll*', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'expired', message: 'Device code expired. Please try again.' }),
        })
      )

      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      const toast = page.locator('div.toast.toast-error').filter({ hasText: 'Device code expired' })
      await expect(toast).toBeVisible({ timeout: POLL_TOAST_TIMEOUT })
    })

    test('access denied shows error toast', async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
      await page.route('**/_ui/api/providers/qwen/device-code', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(MOCK_DEVICE_CODE),
        })
      )

      await page.route('**/_ui/api/providers/qwen/device-poll*', route =>
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'denied', message: 'Authorization was denied.' }),
        })
      )

      await navigateToConnections(page)
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      const toast = page.locator('div.toast.toast-error').filter({ hasText: 'Authorization was denied' })
      await expect(toast).toBeVisible({ timeout: POLL_TOAST_TIMEOUT })
    })
  })

  // --- Disconnect flow ---

  test.describe('Disconnect flow', () => {
    test('clicking disconnect sends DELETE and shows success toast', async ({ page }) => {
      await mockQwenStatus(page, QWEN_CONNECTED)

      let capturedMethod = ''
      await page.route('**/_ui/api/providers/qwen/disconnect', route => {
        if (route.request().method() === 'DELETE') {
          capturedMethod = route.request().method()
          route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
        } else {
          route.continue()
        }
      })

      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await card.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
      await expectToastMessage(page, 'Qwen Coder disconnected', 'success')
      expect(capturedMethod).toBe('DELETE')
    })

    test('shows error toast when disconnect fails', async ({ page }) => {
      await mockQwenStatus(page, QWEN_CONNECTED)

      await page.route('**/_ui/api/providers/qwen/disconnect', route =>
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Internal server error' }),
        })
      )

      await navigateToConnections(page)
      const card = page.locator('div.card').filter({ hasText: 'Qwen Coder' })
      await card.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
      await expectToastMessage(page, 'Failed to disconnect', 'error')
    })
  })

  // --- Loading state ---

  test.describe('Loading state', () => {
    test('shows skeleton loader while fetching Qwen status', async ({ page }) => {
      // Delay the qwen status response to observe loading state
      await page.route('**/_ui/api/providers/qwen/status', async route => {
        await new Promise(r => setTimeout(r, 2000))
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(QWEN_CONNECTED),
        })
      })

      await page.goto('./providers')
      await page.locator('[role="tab"]', { hasText: 'connections' }).click()
      const skeleton = page.locator('[role="status"][aria-label="Loading Qwen status"]')
      await expect(skeleton).toBeVisible()
    })
  })
})
