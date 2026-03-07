import { defineConfig, devices } from '@playwright/test'

/**
 * Minimal config for running tests against the local Vite dev server.
 * No auth session required — only public pages (login).
 */
export default defineConfig({
  testDir: './specs',
  testMatch: '{theme-toggle,provider-oauth}.spec.ts',
  fullyParallel: false,
  retries: 0,
  workers: 1,
  reporter: 'list',
  outputDir: 'test-results',

  use: {
    baseURL: 'http://localhost:5173/_ui/',
    screenshot: 'only-on-failure',
    trace: 'off',
    ...devices['Desktop Chrome'],
  },
})
