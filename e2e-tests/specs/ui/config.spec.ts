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
] as const

test.describe('Config page', () => {
  test('renders all 8 config groups', async ({ page }) => {
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
    expect(count).toBe(8)

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

// ── Authentication config group ──────────────────────────────────────

test.describe('Config page — Authentication group', () => {
  test('Authentication group has Google SSO and Password Auth toggles', async ({ page }) => {
    await navigateTo(page, '/config')

    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    await expect(authGroup).toBeVisible()

    // Google SSO toggle
    const googleLabel = authGroup.locator('label.config-label', { hasText: 'Google SSO' })
    await expect(googleLabel).toBeVisible()

    // Password Auth toggle
    const passwordLabel = authGroup.locator('label.config-label', { hasText: 'Password Auth' })
    await expect(passwordLabel).toBeVisible()
  })

  test('admin can toggle and save an auth config field', async ({ page }) => {
    await navigateTo(page, '/config')

    // Find password auth checkbox within the Authentication group
    const authGroup = page.locator('div.config-group').filter({ hasText: 'Authentication' })
    const input = authGroup.locator('input#auth_password_enabled')
    await expect(input).toBeVisible()

    const wasBefore = await input.isChecked()

    // Toggle
    await input.click()

    // Save
    const saveBtn = page.locator(Form.save, { hasText: 'Save Configuration' })
    await saveBtn.click()
    await expectToastMessage(page, 'applied immediately')

    // Toggle back
    await navigateTo(page, '/config')
    const input2 = page.locator('div.config-group').filter({ hasText: 'Authentication' })
      .locator('input#auth_password_enabled')
    if (wasBefore !== await input2.isChecked()) {
      // Already toggled back? No action needed
    } else {
      await input2.click()
      await page.locator(Form.save, { hasText: 'Save Configuration' }).click()
      await expectToastMessage(page, 'applied immediately')
    }
  })

  test('unsaved changes indicator appears when editing', async ({ page }) => {
    await navigateTo(page, '/config')

    // Find a text input to edit (e.g. server_port or similar in Server group)
    const serverGroup = page.locator('div.config-group').filter({ hasText: 'Server' })
    const inputs = serverGroup.locator('input.config-input[type="text"], input.config-input[type="number"]')
    const count = await inputs.count()
    test.skip(count === 0, 'No editable text/number inputs in Server group')

    const input = inputs.first()
    const originalValue = await input.inputValue()

    // Type something different
    await input.clear()
    await input.fill(originalValue + '0')

    // Unsaved changes indicator should appear
    const unsavedText = page.locator('div.config-save-bar')
    await expect(unsavedText).toBeVisible()

    // Revert by reloading (don't save)
    await navigateTo(page, '/config')
  })
})

// ── Config page — Change History ────────────────────────────────────

test.describe('Config page — change history panel', () => {
  test('history panel is visible on config page', async ({ page }) => {
    await navigateTo(page, '/config')

    const historyPanel = page.locator('div.history-panel')
    await expect(historyPanel).toBeVisible()
  })
})
