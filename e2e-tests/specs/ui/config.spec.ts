import { test, expect } from '@playwright/test'
import { Form } from '../../helpers/selectors.js'
import { navigateTo, expectToastMessage } from '../../helpers/navigation.js'

const CONFIG_GROUPS = [
  'Server',
  'Kiro Backend',
  'Timeouts',
  'Debug',
  'Converter',
  'HTTP Client',
  'Features',
  'Authentication',
  'Provider OAuth',
] as const

const PROVIDER_OAUTH_FIELDS = [
  { key: 'qwen_oauth_client_id', label: 'Qwen OAuth Client ID' },
  { key: 'anthropic_oauth_client_id', label: 'Anthropic OAuth Client ID' },
  { key: 'openai_oauth_client_id', label: 'OpenAI OAuth Client ID' },
] as const

test.describe('Config page', () => {
  test('renders all 9 config groups', async ({ page }) => {
    await navigateTo(page, '/config')

    for (const group of CONFIG_GROUPS) {
      const header = page.locator('h3.config-group-header', { hasText: group })
      await expect(header).toBeVisible()
    }
  })

  test('each group has config inputs', async ({ page }) => {
    await navigateTo(page, '/config')

    const groups = page.locator('div.config-group')
    const count = await groups.count()
    expect(count).toBe(9)

    // Each group should have at least one config input
    for (let i = 0; i < count; i++) {
      const group = groups.nth(i)
      const inputs = group.locator('.config-input')
      const inputCount = await inputs.count()
      expect(inputCount).toBeGreaterThan(0)
    }
  })

  test('save button is present', async ({ page }) => {
    await navigateTo(page, '/config')

    const saveBtn = page.locator(Form.save, { hasText: 'Save Configuration' })
    await expect(saveBtn).toBeVisible()
  })
})

// ── Provider OAuth config group ─────────────────────────────────────

test.describe('Config page — Provider OAuth group', () => {
  test('Provider OAuth group is visible with three fields', async ({ page }) => {
    await navigateTo(page, '/config')

    // Group header visible
    const header = page.locator('h3.config-group-header', { hasText: 'Provider OAuth' })
    await expect(header).toBeVisible()

    // Find the Provider OAuth group container
    const group = page.locator('div.config-group').filter({ hasText: 'Provider OAuth' })
    await expect(group).toBeVisible()

    // All three fields must be present with their labels
    for (const field of PROVIDER_OAUTH_FIELDS) {
      const label = group.locator('label.config-label', { hasText: field.label })
      await expect(label).toBeVisible()
    }

    // Group should have exactly 3 text inputs
    const inputs = group.locator('input.config-input[type="text"]')
    const count = await inputs.count()
    expect(count).toBe(3)
  })

  test('all three Provider OAuth fields show "live" badge (HotReload)', async ({ page }) => {
    await navigateTo(page, '/config')

    const group = page.locator('div.config-group').filter({ hasText: 'Provider OAuth' })

    for (const field of PROVIDER_OAUTH_FIELDS) {
      const row = group.locator('div.config-row').filter({ hasText: field.label })
      await expect(row).toBeVisible()

      // Should have "live" badge, not "restart"
      const liveBadge = row.locator('span', { hasText: 'live' })
      await expect(liveBadge).toBeVisible()

      const restartBadge = row.locator('span.badge-restart')
      await expect(restartBadge).not.toBeVisible()
    }
  })

  test('admin can edit and save a provider OAuth client ID', async ({ page }) => {
    await navigateTo(page, '/config')

    const testValue = `e2e-test-${Date.now()}`

    // Fill in the Qwen OAuth Client ID field
    const input = page.locator('input#qwen_oauth_client_id')
    await expect(input).toBeVisible()
    await input.clear()
    await input.fill(testValue)

    // Submit the form
    const saveBtn = page.locator(Form.save, { hasText: 'Save Configuration' })
    await saveBtn.click()

    // Expect success toast (hot-reload, no restart needed)
    await expectToastMessage(page, 'applied immediately')

    // Reload and verify the value persisted
    await navigateTo(page, '/config')
    const reloadedInput = page.locator('input#qwen_oauth_client_id')
    await expect(reloadedInput).toHaveValue(testValue)
  })

  test('unsaved changes indicator appears when editing', async ({ page }) => {
    await navigateTo(page, '/config')

    // Get current value to restore later
    const input = page.locator('input#anthropic_oauth_client_id')
    await expect(input).toBeVisible()
    const originalValue = await input.inputValue()

    // Type something different
    await input.clear()
    await input.fill('modified-value-for-dirty-check')

    // Unsaved dot indicator should appear
    const unsavedDot = page.locator('span.unsaved-dot')
    await expect(unsavedDot).toBeVisible()

    // Revert by reloading (don't save)
    await navigateTo(page, '/config')

    // Value should be unchanged (the original value)
    const restoredInput = page.locator('input#anthropic_oauth_client_id')
    await expect(restoredInput).toHaveValue(originalValue)
  })
})

// ── Config page — Change History ────────────────────────────────────

test.describe('Config page — change history panel', () => {
  test('history panel shows provider OAuth changes after save', async ({ page }) => {
    await navigateTo(page, '/config')

    const marker = `history-ui-e2e-${Date.now()}`

    // Edit and save a provider OAuth field
    const input = page.locator('input#openai_oauth_client_id')
    await expect(input).toBeVisible()
    await input.clear()
    await input.fill(marker)

    const saveBtn = page.locator(Form.save, { hasText: 'Save Configuration' })
    await saveBtn.click()

    // Wait for toast confirming save
    await expectToastMessage(page, 'applied immediately')

    // History panel should contain the change
    const historyPanel = page.locator('div.history-panel')
    await expect(historyPanel).toBeVisible()

    // Look for the key name in a history item
    const historyEntry = historyPanel.locator('div.history-item', {
      hasText: 'openai_oauth_client_id',
    })
    await expect(historyEntry).toBeVisible({ timeout: 5_000 })

    // The new value should appear in the diff
    const newVal = historyEntry.locator('span.new-val', { hasText: marker })
    await expect(newVal).toBeVisible()
  })
})
