import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Status } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

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

async function mockKiroStatus(page: Page) {
  await page.route('**/_ui/api/kiro/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ has_token: false, expired: false }),
    })
  )
}

async function mockCopilotStatus(page: Page) {
  await page.route('**/_ui/api/copilot/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ connected: false, github_username: null, copilot_plan: null, expired: false }),
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

async function mockQwenStatus(page: Page, data: QwenStatusData = QWEN_CONNECTED) {
  await page.route('**/_ui/api/providers/qwen/status', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(data),
    })
  )
}

async function mockAllProfileDeps(page: Page) {
  await mockSession(page)
  await mockKiroStatus(page)
  await mockCopilotStatus(page)
  await mockApiKeys(page)
  await mockProvidersStatus(page)
}

// --- Tests ---

test.describe('QwenSetup component on Profile page', () => {
  test.beforeEach(async ({ page }) => {
    await mockAllProfileDeps(page)
  })

  // --- 7.1: Section structure ---

  test.describe('Section structure', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
    })

    test('renders QWEN CODER section header', async ({ page }) => {
      await navigateTo(page, '/profile')
      const header = page.locator('h2.section-header', { hasText: 'QWEN CODER' })
      await expect(header).toBeVisible()
    })

    test('renders card with title "qwen coder"', async ({ page }) => {
      await navigateTo(page, '/profile')
      const title = page.locator('span.card-title', { hasText: 'qwen coder' })
      await expect(title).toBeVisible()
    })

    test('card structure follows KiroSetup/CopilotSetup pattern', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      const card = section.locator('div.card')
      await expect(card).toBeVisible()
      await expect(card.locator('.card-header')).toBeVisible()
    })
  })

  // --- 7.2: Connected state ---

  test.describe('Connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_CONNECTED)
    })

    test('shows CONNECTED badge', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      const card = section.first()
      await expect(card.locator(Status.ok)).toBeVisible()
      await expect(card.locator(Status.ok)).toContainText('CONNECTED')
    })

    test('shows "$ reconnect" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('button.btn-save', { hasText: '$ reconnect' })).toBeVisible()
    })

    test('shows "disconnect" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      await expect(section.locator('button.device-code-cancel', { hasText: 'disconnect' })).toBeVisible()
    })

    test('does not show "$ connect qwen" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('button.btn-save', { hasText: '$ connect qwen' })).not.toBeAttached()
    })
  })

  // --- 7.3: Not connected state ---

  test.describe('Not connected state', () => {
    test.beforeEach(async ({ page }) => {
      await mockQwenStatus(page, QWEN_DISCONNECTED)
    })

    test('shows NOT CONNECTED badge', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      const card = section.first()
      await expect(card.locator(Status.err)).toBeVisible()
      await expect(card.locator(Status.err)).toContainText('NOT CONNECTED')
    })

    test('shows "$ connect qwen" button', async ({ page }) => {
      await navigateTo(page, '/profile')
      await expect(page.locator('button.btn-save', { hasText: '$ connect qwen' })).toBeVisible()
    })

    test('does not show disconnect button', async ({ page }) => {
      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      await expect(section.locator('button.device-code-cancel', { hasText: 'disconnect' })).not.toBeAttached()
    })
  })

  // --- 7.4: Device flow initiation ---

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
      // Mock poll endpoint to keep returning pending (prevent test from completing prematurely)
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

      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      expect(capturedMethod).toBe('POST')
    })

    test('shows verification URL as clickable link', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      const link = page.locator('a.device-code-link')
      await expect(link).toBeVisible()
      await expect(link).toHaveAttribute('href', MOCK_DEVICE_CODE.verification_uri_complete)
      await expect(link).toHaveAttribute('target', '_blank')
    })

    test('shows user code', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-value')).toContainText('QWEN-1234')
    })

    test('shows copy button', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-copy')).toContainText('[click to copy]')
    })

    test('shows polling indicator', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-polling')).toContainText('polling...')
    })

    test('shows cancel button', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('button.device-code-cancel', { hasText: '$ cancel' })).toBeVisible()
    })

    test('cancel closes device flow and returns to card', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-wrap')).toBeVisible()
      await page.locator('button.device-code-cancel', { hasText: '$ cancel' }).click()
      await expect(page.locator('.device-code-wrap')).not.toBeAttached()
      // Card should be back
      await expect(page.locator('span.card-title', { hasText: 'qwen coder' })).toBeVisible()
    })

    test('shows verification URI text', async ({ page }) => {
      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expect(page.locator('.device-code-uri')).toContainText('https://chat.qwen.ai/device')
    })
  })

  // --- 7.5: Device flow polling states ---

  test.describe('Device flow polling states', () => {
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

      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expectToastMessage(page, 'Qwen Coder connected successfully', 'success')
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

      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expectToastMessage(page, 'Device code expired', 'error')
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

      await navigateTo(page, '/profile')
      await page.locator('button.btn-save', { hasText: '$ connect qwen' }).click()
      await expectToastMessage(page, 'Authorization was denied', 'error')
    })
  })

  // --- 7.6: Disconnect flow ---

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

      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      await section.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
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

      await navigateTo(page, '/profile')
      const section = page.locator('h2.section-header', { hasText: 'QWEN CODER' }).locator('~ div').first()
      await section.locator('button.device-code-cancel', { hasText: 'disconnect' }).click()
      await expectToastMessage(page, 'Failed to disconnect', 'error')
    })
  })

  // --- 7.7: Loading state ---

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

      await page.goto('./profile')
      const skeleton = page.locator('[role="status"][aria-label="Loading Qwen status"]')
      await expect(skeleton).toBeVisible()
    })
  })
})
