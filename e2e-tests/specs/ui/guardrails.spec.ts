import { test, expect } from '@playwright/test'
import type { Page } from '@playwright/test'
import { Card, Form } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

// ── Rendering tests ─────────────────────────────────────────────────

test.describe('Guardrails page', () => {
  test('renders three sections: PROFILES, RULES, TEST GUARDRAIL', async ({ page }) => {
    await navigateTo(page, '/guardrails')

    const profilesHeader = page.locator('h2.section-header', { hasText: 'PROFILES' })
    await expect(profilesHeader).toBeVisible()

    const rulesHeader = page.locator('h2.section-header', { hasText: 'RULES' })
    await expect(rulesHeader).toBeVisible()

    const testHeader = page.locator('h2.section-header', { hasText: 'TEST GUARDRAIL' })
    await expect(testHeader).toBeVisible()
  })

  test('new profile button is visible', async ({ page }) => {
    await navigateTo(page, '/guardrails')

    const newProfileBtn = page.locator(Form.save, { hasText: '$ new profile' })
    await expect(newProfileBtn).toBeVisible()
  })

  test('new rule button is visible', async ({ page }) => {
    await navigateTo(page, '/guardrails')

    const newRuleBtn = page.locator(Form.save, { hasText: '$ new rule' })
    await expect(newRuleBtn).toBeVisible()
  })

  test('profiles card renders', async ({ page }) => {
    await navigateTo(page, '/guardrails')

    const profilesTitle = page.locator(Card.title, { hasText: 'profiles' })
    await expect(profilesTitle).toBeVisible()
  })

  test('rules card renders', async ({ page }) => {
    await navigateTo(page, '/guardrails')

    const rulesTitle = page.locator(Card.title, { hasText: 'rules' })
    await expect(rulesTitle).toBeVisible()
  })

  test('test card renders', async ({ page }) => {
    await navigateTo(page, '/guardrails')

    const testTitle = page.locator(Card.title, { hasText: 'test' })
    await expect(testTitle).toBeVisible()
  })
})

// ── Mocked functional tests ─────────────────────────────────────────

const MOCK_PROFILE = {
  id: 'mock-profile-001',
  name: 'E2E Test Profile',
  guardrail_id: 'gr-test-001',
  guardrail_version: '1',
  region: 'us-east-1',
  access_key: 'AKIA...',
  secret_key: '',
  enabled: true,
}

async function mockGuardrailsAPIs(page: Page) {
  // Mock profiles list
  await page.route('**/_ui/api/guardrails/profiles', route => {
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([MOCK_PROFILE]),
      })
    } else if (route.request().method() === 'POST') {
      route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({ ...MOCK_PROFILE, id: 'new-profile-id' }),
      })
    } else {
      route.continue()
    }
  })

  // Mock rules list
  await page.route('**/_ui/api/guardrails/rules', route => {
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([]),
      })
    } else if (route.request().method() === 'POST') {
      route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({ id: 'new-rule-id' }),
      })
    } else {
      route.continue()
    }
  })

  // Mock CEL validation
  await page.route('**/_ui/api/guardrails/validate-cel', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ valid: true }),
    })
  )

  // Mock test endpoint
  await page.route('**/_ui/api/guardrails/profiles/*/test', route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ action: 'NONE', response_time_ms: 42 }),
    })
  )
}

test.describe('Guardrails page — profile form (mocked)', () => {
  test('clicking new profile opens form with correct fields', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    const newProfileBtn = page.locator(Form.save, { hasText: '$ new profile' })
    await newProfileBtn.click()

    // Form should appear with expected fields
    await expect(page.locator('label[for="profile-name"]')).toBeVisible()
    await expect(page.locator('label[for="profile-gid"]')).toBeVisible()
    await expect(page.locator('label[for="profile-ver"]')).toBeVisible()
    await expect(page.locator('label[for="profile-region"]')).toBeVisible()
    await expect(page.locator('label[for="profile-ak"]')).toBeVisible()
    await expect(page.locator('label[for="profile-sk"]')).toBeVisible()

    // Create and cancel buttons visible
    await expect(page.locator('button.btn-save', { hasText: '$ create' })).toBeVisible()
    await expect(page.locator('button.device-code-cancel', { hasText: 'cancel' })).toBeVisible()
  })

  test('submitting profile form shows success toast', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    // Open form
    await page.locator(Form.save, { hasText: '$ new profile' }).click()

    // Fill required fields
    await page.locator('input#profile-name').fill('Test Profile')
    await page.locator('input#profile-gid').fill('gr-12345')
    await page.locator('input#profile-ak').fill('AKIATEST')
    await page.locator('input#profile-sk').fill('secret-key-here')

    // Submit
    await page.locator('button.btn-save', { hasText: '$ create' }).click()

    await expectToastMessage(page, 'Profile created')
  })

  test('cancel button closes form', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    await page.locator(Form.save, { hasText: '$ new profile' }).click()
    await expect(page.locator('input#profile-name')).toBeVisible()

    await page.locator('button.device-code-cancel', { hasText: 'cancel' }).click()
    await expect(page.locator('input#profile-name')).not.toBeVisible()
  })
})

test.describe('Guardrails page — rule form (mocked)', () => {
  test('clicking new rule opens form with CEL textarea', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    await page.locator(Form.save, { hasText: '$ new rule' }).click()

    // Rule form fields
    await expect(page.locator('label[for="rule-name"]')).toBeVisible()
    await expect(page.locator('label[for="rule-cel"]')).toBeVisible()
    await expect(page.locator('textarea#rule-cel')).toBeVisible()
    await expect(page.locator('label[for="rule-apply"]')).toBeVisible()

    // Validate button next to CEL textarea
    await expect(page.locator('button.btn-save', { hasText: 'validate' })).toBeVisible()
  })

  test('validate button shows valid status for CEL expression', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    await page.locator(Form.save, { hasText: '$ new rule' }).click()

    // Fill CEL expression
    const celInput = page.locator('textarea#rule-cel')
    await celInput.fill('request.model == "claude-sonnet-4-20250514"')

    // Click validate
    await page.locator('button.btn-save', { hasText: 'validate' }).click()

    // Should show "valid" status
    const statusDiv = page.locator('div[aria-live="polite"]')
    await expect(statusDiv.locator('div', { hasText: 'valid' })).toBeVisible()
  })
})

test.describe('Guardrails page — test panel (mocked)', () => {
  test('test panel has profile selector, content area, and test button', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    // Profile dropdown
    const profileSelect = page.locator('select#test-profile')
    await expect(profileSelect).toBeVisible()

    // Content textarea
    const contentArea = page.locator('textarea#test-content')
    await expect(contentArea).toBeVisible()

    // Test button
    const testBtn = page.locator('button.btn-save', { hasText: '$ test' })
    await expect(testBtn).toBeVisible()
  })

  test('running test shows result with action and time', async ({ page }) => {
    await mockGuardrailsAPIs(page)
    await navigateTo(page, '/guardrails')

    // Fill content
    const contentArea = page.locator('textarea#test-content')
    await contentArea.fill('This is a test message for guardrail validation.')

    // Click test
    await page.locator('button.btn-save', { hasText: '$ test' }).click()

    // Result should appear
    const result = page.locator('div.guardrails-test-result')
    await expect(result).toBeVisible({ timeout: 5_000 })
    await expect(result.locator('span', { hasText: 'action: NONE' })).toBeVisible()
    await expect(result.locator('span', { hasText: /time: \d+ms/ })).toBeVisible()
  })
})
