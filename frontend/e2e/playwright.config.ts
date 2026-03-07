import { defineConfig, devices } from '@playwright/test'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const authDir = path.join(__dirname, '.auth')

export default defineConfig({
  testDir: './specs',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [['html', { outputFolder: 'playwright-report' }]],
  outputDir: 'test-results',
  globalSetup: './global-setup.ts',

  use: {
    baseURL: 'https://localhost/_ui/',
    ignoreHTTPSErrors: true,
    screenshot: 'only-on-failure',
    trace: 'on-first-retry',
    ...devices['Desktop Chrome'],
  },

  projects: [
    {
      name: 'public',
      testMatch: ['login.spec.ts', 'auth-redirect.spec.ts'],
      use: { storageState: undefined },
    },
    {
      name: 'authenticated',
      testMatch: ['dashboard.spec.ts', 'profile.spec.ts', 'navigation.spec.ts', 'providers.spec.ts'],
      use: { storageState: path.join(authDir, 'session.json') },
    },
    {
      name: 'admin',
      testMatch: ['config.spec.ts', 'admin.spec.ts', 'guardrails.spec.ts', 'mcp.spec.ts'],
      use: { storageState: path.join(authDir, 'session.json') },
    },
  ],
})
