import { test, expect } from '@playwright/test'

const SCREENSHOT_DIR = '/Users/hikennoace/ai-gateway/harbangan/.playwright-mcp'
const BASE_URL = 'http://localhost:5173/_ui/'

test.describe('Theme toggle – Light/Dark mode', () => {
  // ── Test 2.1: Visual Verification (both modes) ──────────────────────

  test('2.1a – dark mode renders correctly on login page', async ({ browser }) => {
    // Emulate dark color scheme so ThemeProvider defaults to dark
    const context = await browser.newContext({ colorScheme: 'dark', baseURL: BASE_URL })
    const page = await context.newPage()

    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    // Confirm data-theme is dark
    const theme = await page.evaluate(() =>
      document.documentElement.getAttribute('data-theme')
    )
    expect(theme).toBe('dark')

    // Verify dark mode CSS variables
    const vars = await page.evaluate(() => {
      const root = document.documentElement
      const getVar = (name: string) => getComputedStyle(root).getPropertyValue(name).trim()
      return {
        bg: getVar('--bg'),
        text: getVar('--text'),
        green: getVar('--green'),
        surface: getVar('--surface'),
      }
    })
    expect(vars.bg).toBe('#060609')
    expect(vars.text).toBe('#b8ccb8')
    expect(vars.green).toBe('#4ade80')
    expect(vars.surface).toBe('#101018')

    // Verify scanlines (body::before) are visible in dark mode
    const scanlineDisplay = await page.evaluate(() => {
      const style = getComputedStyle(document.body, '::before')
      return style.display
    })
    expect(scanlineDisplay).not.toBe('none')

    await page.screenshot({ path: `${SCREENSHOT_DIR}/dark-mode-login.png`, fullPage: true })

    await context.close()
  })

  test('2.1b – light mode renders correctly on login page', async ({ browser }) => {
    // Emulate light color scheme so ThemeProvider defaults to light
    const context = await browser.newContext({ colorScheme: 'light', baseURL: BASE_URL })
    const page = await context.newPage()

    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    // Confirm data-theme is light
    const theme = await page.evaluate(() =>
      document.documentElement.getAttribute('data-theme')
    )
    expect(theme).toBe('light')

    // Verify light mode CSS variables
    const vars = await page.evaluate(() => {
      const root = document.documentElement
      const getVar = (name: string) => getComputedStyle(root).getPropertyValue(name).trim()
      return {
        bg: getVar('--bg'),
        text: getVar('--text'),
        green: getVar('--green'),
        surface: getVar('--surface'),
        theme: root.getAttribute('data-theme'),
      }
    })
    expect(vars.bg).toBe('#f8f8f5')
    expect(vars.text).toBe('#1a2a1a')
    expect(vars.green).toBe('#16a34a')
    expect(vars.surface).toBe('#f0f0ec')
    expect(vars.theme).toBe('light')

    // Verify scanlines/vignette are hidden in light mode
    const scanlineDisplay = await page.evaluate(() => {
      const style = getComputedStyle(document.body, '::before')
      return style.display
    })
    expect(scanlineDisplay).toBe('none')

    await page.screenshot({ path: `${SCREENSHOT_DIR}/light-mode-login.png`, fullPage: true })

    await context.close()
  })

  test('2.1c – dark mode restores correctly after toggling', async ({ browser }) => {
    // Start with dark scheme
    const context = await browser.newContext({ colorScheme: 'dark', baseURL: BASE_URL })
    const page = await context.newPage()

    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    // Switch to light then back to dark
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'light')
    })
    await page.waitForTimeout(200)
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'dark')
    })
    await page.waitForTimeout(300)

    // Verify dark mode is restored
    const vars = await page.evaluate(() => {
      const root = document.documentElement
      const getVar = (name: string) => getComputedStyle(root).getPropertyValue(name).trim()
      return {
        bg: getVar('--bg'),
        text: getVar('--text'),
        green: getVar('--green'),
        theme: root.getAttribute('data-theme'),
      }
    })
    expect(vars.bg).toBe('#060609')
    expect(vars.text).toBe('#b8ccb8')
    expect(vars.green).toBe('#4ade80')
    expect(vars.theme).toBe('dark')

    // Verify scanlines are back
    const scanlineDisplay = await page.evaluate(() => {
      const style = getComputedStyle(document.body, '::before')
      return style.display
    })
    expect(scanlineDisplay).not.toBe('none')

    await page.screenshot({ path: `${SCREENSHOT_DIR}/dark-mode-restored.png`, fullPage: true })

    await context.close()
  })

  // ── Test 2.2: Toggle Persistence ─────────────────────────────────────

  test('2.2 – theme persists across page navigation via localStorage', async ({ browser }) => {
    // Start with dark scheme, then set light via localStorage
    const context = await browser.newContext({ colorScheme: 'dark', baseURL: BASE_URL })
    const page = await context.newPage()

    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    // Confirm starts as dark
    const initialTheme = await page.evaluate(() =>
      document.documentElement.getAttribute('data-theme')
    )
    expect(initialTheme).toBe('dark')

    // Set light mode via both DOM and localStorage (simulating what ThemeProvider does)
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'light')
      localStorage.setItem('harbangan-theme', 'light')
    })
    await page.waitForTimeout(200)

    await page.screenshot({ path: `${SCREENSHOT_DIR}/light-mode-set.png`, fullPage: true })

    // Navigate away and back (re-navigate to login)
    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    // Check persistence — ThemeProvider should read 'light' from localStorage
    const state = await page.evaluate(() =>
      JSON.stringify({
        theme: document.documentElement.getAttribute('data-theme'),
        stored: localStorage.getItem('harbangan-theme'),
      })
    )
    const parsed = JSON.parse(state)

    await page.screenshot({ path: `${SCREENSHOT_DIR}/light-mode-persisted.png`, fullPage: true })

    // Theme should persist via ThemeProvider reading localStorage
    expect(parsed.stored).toBe('light')
    expect(parsed.theme).toBe('light')

    await context.close()
  })

  // ── Test 2.3: CSS Variable Verification ──────────────────────────────

  test('2.3 – CSS variables differ correctly between dark and light modes', async ({ browser }) => {
    // Start with dark color scheme
    const context = await browser.newContext({ colorScheme: 'dark', baseURL: BASE_URL })
    const page = await context.newPage()

    await page.goto('./login')
    await page.waitForLoadState('networkidle')

    // Read dark mode variables
    const darkVars = await page.evaluate(() => {
      const root = document.documentElement
      const getVar = (name: string) => getComputedStyle(root).getPropertyValue(name).trim()
      return {
        bg: getVar('--bg'),
        text: getVar('--text'),
        green: getVar('--green'),
        surface: getVar('--surface'),
        bgRaised: getVar('--bg-raised'),
        border: getVar('--border'),
        cyan: getVar('--cyan'),
        red: getVar('--red'),
        yellow: getVar('--yellow'),
        blue: getVar('--blue'),
        theme: root.getAttribute('data-theme'),
      }
    })

    // Switch to light mode
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'light')
    })
    await page.waitForTimeout(300)

    // Read light mode variables
    const lightVars = await page.evaluate(() => {
      const root = document.documentElement
      const getVar = (name: string) => getComputedStyle(root).getPropertyValue(name).trim()
      return {
        bg: getVar('--bg'),
        text: getVar('--text'),
        green: getVar('--green'),
        surface: getVar('--surface'),
        bgRaised: getVar('--bg-raised'),
        border: getVar('--border'),
        cyan: getVar('--cyan'),
        red: getVar('--red'),
        yellow: getVar('--yellow'),
        blue: getVar('--blue'),
        theme: root.getAttribute('data-theme'),
      }
    })

    // Verify dark mode values
    expect(darkVars.theme).toBe('dark')
    expect(darkVars.bg).toBe('#060609')
    expect(darkVars.text).toBe('#b8ccb8')
    expect(darkVars.green).toBe('#4ade80')
    expect(darkVars.surface).toBe('#101018')
    expect(darkVars.bgRaised).toBe('#0b0b10')
    expect(darkVars.border).toBe('#1a1a26')
    expect(darkVars.cyan).toBe('#22d3ee')
    expect(darkVars.red).toBe('#f87171')
    expect(darkVars.yellow).toBe('#fbbf24')
    expect(darkVars.blue).toBe('#60a5fa')

    // Verify light mode values
    expect(lightVars.theme).toBe('light')
    expect(lightVars.bg).toBe('#f8f8f5')
    expect(lightVars.text).toBe('#1a2a1a')
    expect(lightVars.green).toBe('#16a34a')
    expect(lightVars.surface).toBe('#f0f0ec')
    expect(lightVars.bgRaised).toBe('#fff')
    expect(lightVars.border).toBe('#d5d5cf')
    expect(lightVars.cyan).toBe('#0891b2')
    expect(lightVars.red).toBe('#dc2626')
    expect(lightVars.yellow).toBe('#d97706')
    expect(lightVars.blue).toBe('#2563eb')

    // Ensure all values actually differ between modes
    expect(darkVars.bg).not.toBe(lightVars.bg)
    expect(darkVars.text).not.toBe(lightVars.text)
    expect(darkVars.green).not.toBe(lightVars.green)
    expect(darkVars.surface).not.toBe(lightVars.surface)

    await context.close()
  })
})
