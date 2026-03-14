import { test, expect } from '@playwright/test'
import { Form } from '../../helpers/selectors.js'
import { navigateTo } from '../../helpers/navigation.js'

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
