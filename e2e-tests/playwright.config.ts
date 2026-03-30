import { defineConfig, devices } from '@playwright/test';

require('dotenv').config({ path: '../.env' });

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:9999';
const BASE_UI_URL = (process.env.BASE_UI_URL || 'http://localhost:5173/_ui').replace(/\/?$/, '/');
const API_KEY = process.env.API_KEY || '';

export default defineConfig({
  testDir: './specs',
  timeout: 30_000,
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'list',
  outputDir: 'test-results',
  globalSetup: './global-setup.ts',

  projects: [
    // ── Backend API tests (no browser) ──
    {
      name: 'api',
      testDir: './specs/api',
      testIgnore: [
        'api-keys.spec.ts',
        'config.spec.ts',
        'logout.spec.ts',
        'domain-allowlist.spec.ts',
        'multi-account.spec.ts',
        'user-management.spec.ts',
        'guardrails.spec.ts',
        'model-registry.spec.ts',
        'provider-status.spec.ts',
        'provider-toggle.spec.ts',
        'password-auth.spec.ts',
        'sso-config.spec.ts',
      ],
      use: {
        baseURL: GATEWAY_URL,
        extraHTTPHeaders: {
          'Authorization': `Bearer ${API_KEY}`,
        },
      },
    },

    // ── UI: public pages (no auth) ──
    {
      name: 'ui-public',
      testDir: './specs/ui',
      testMatch: ['login.spec.ts', 'auth-redirect.spec.ts', 'theme-toggle.spec.ts', 'password-login.spec.ts'],
      use: {
        baseURL: BASE_UI_URL,
        ignoreHTTPSErrors: true,
        storageState: undefined,
        screenshot: 'only-on-failure',
        trace: 'on-first-retry',
        ...devices['Desktop Chrome'],
      },
    },

    // ── API: mutating endpoints (serial — one file, one test at a time) ──
    {
      name: 'api-mutating',
      dependencies: ['api', 'ui-public', 'ui-authenticated'],
      testDir: './specs/api',
      testMatch: [
        'api-keys.spec.ts',
        'config.spec.ts',
        'logout.spec.ts',
        'domain-allowlist.spec.ts',
        'multi-account.spec.ts',
        'user-management.spec.ts',
        'guardrails.spec.ts',
        'model-registry.spec.ts',
        'provider-status.spec.ts',
        'provider-toggle.spec.ts',
        'password-auth.spec.ts',
        'sso-config.spec.ts',
      ],
      fullyParallel: false,
      use: {
        baseURL: GATEWAY_URL,
        storageState: '.auth/session.json',
      },
    },

    // ── UI: authenticated pages (read-only rendering, parallel) ──
    {
      name: 'ui-authenticated',
      testDir: './specs/ui',
      testMatch: [
        'dashboard.spec.ts', 'profile.spec.ts', 'navigation.spec.ts',
        'provider-oauth.spec.ts', 'copilot-setup.spec.ts',
        'totp-setup.spec.ts', 'password-change.spec.ts',
        'usage.spec.ts',
      ],
      use: {
        baseURL: BASE_UI_URL,
        ignoreHTTPSErrors: true,
        storageState: '.auth/session.json',
        screenshot: 'only-on-failure',
        trace: 'on-first-retry',
        ...devices['Desktop Chrome'],
      },
    },

    // ── UI: admin pages (serial — mutations on shared state) ──
    {
      name: 'ui-admin',
      dependencies: ['api-mutating'],
      testDir: './specs/ui',
      testMatch: [
        'config.spec.ts', 'admin.spec.ts', 'admin-users.spec.ts',
        'guardrails.spec.ts', 'multi-account.spec.ts',
        'user-detail.spec.ts', 'logout-redirect.spec.ts', 'profile-actions.spec.ts',
        'sso-config-flow.spec.ts',
      ],
      fullyParallel: false,
      use: {
        baseURL: BASE_UI_URL,
        ignoreHTTPSErrors: true,
        storageState: '.auth/session.json',
        screenshot: 'only-on-failure',
        trace: 'on-first-retry',
        ...devices['Desktop Chrome'],
      },
    },
  ],
});
