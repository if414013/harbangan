import { defineConfig } from '@playwright/test';

require('dotenv').config({ path: '../.env' });

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:9999';

/**
 * Separate config for setup-only mode tests.
 * No globalSetup — runs against a backend with an empty DB (no admin seeded).
 */
export default defineConfig({
  testDir: './specs/api',
  timeout: 30_000,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: 'list',
  outputDir: 'test-results',

  projects: [
    {
      name: 'setup-mode',
      testDir: './specs/api',
      testMatch: ['setup-mode.spec.ts'],
      use: {
        baseURL: GATEWAY_URL,
      },
    },
  ],
});
