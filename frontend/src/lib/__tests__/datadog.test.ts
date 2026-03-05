import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock the Datadog RUM SDK before importing the module under test
vi.mock('@datadog/browser-rum', () => ({
  datadogRum: {
    init: vi.fn(),
  },
}))

import { datadogRum } from '@datadog/browser-rum'

describe('initDatadog', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.resetModules()
    // Stub window.location for Node test environment
    vi.stubGlobal('window', {
      location: { origin: 'http://localhost' },
    })
  })

  it('does not call datadogRum.init when VITE_DD_CLIENT_TOKEN is missing', async () => {
    vi.stubEnv('VITE_DD_CLIENT_TOKEN', '')
    vi.stubEnv('VITE_DD_APPLICATION_ID', 'app-123')
    vi.resetModules()
    const { initDatadog } = await import('../datadog')
    initDatadog()
    expect(datadogRum.init).not.toHaveBeenCalled()
  })

  it('does not call datadogRum.init when VITE_DD_APPLICATION_ID is missing', async () => {
    vi.stubEnv('VITE_DD_CLIENT_TOKEN', 'tok-123')
    vi.stubEnv('VITE_DD_APPLICATION_ID', '')
    vi.resetModules()
    const { initDatadog } = await import('../datadog')
    initDatadog()
    expect(datadogRum.init).not.toHaveBeenCalled()
  })

  it('calls datadogRum.init with correct config when both env vars are set', async () => {
    vi.stubEnv('VITE_DD_CLIENT_TOKEN', 'tok-abc')
    vi.stubEnv('VITE_DD_APPLICATION_ID', 'app-xyz')
    vi.stubEnv('VITE_DD_ENV', 'staging')
    vi.stubEnv('VITE_DD_SITE', 'datadoghq.eu')
    vi.resetModules()
    const { initDatadog } = await import('../datadog')
    initDatadog()
    expect(datadogRum.init).toHaveBeenCalledOnce()
    const config = vi.mocked(datadogRum.init).mock.calls[0][0]
    expect(config.clientToken).toBe('tok-abc')
    expect(config.applicationId).toBe('app-xyz')
    expect(config.env).toBe('staging')
    expect(config.site).toBe('datadoghq.eu')
    expect(config.sessionReplaySampleRate).toBe(0)
  })

  it('allowedTracingUrls restricts to same-origin paths', async () => {
    vi.stubEnv('VITE_DD_CLIENT_TOKEN', 'tok-abc')
    vi.stubEnv('VITE_DD_APPLICATION_ID', 'app-xyz')
    vi.resetModules()
    const { initDatadog } = await import('../datadog')
    initDatadog()
    const config = vi.mocked(datadogRum.init).mock.calls[0][0]
    const urls = config.allowedTracingUrls as Array<(url: string) => boolean>

    // Should match same-origin API paths
    expect(urls.some(fn => fn('http://localhost/_ui/api/metrics'))).toBe(true)
    expect(urls.some(fn => fn('http://localhost/v1/chat/completions'))).toBe(true)

    // Should NOT match third-party APIs
    expect(urls.every(fn => !fn('https://api.openai.com/v1/chat/completions'))).toBe(true)
    expect(urls.every(fn => !fn('https://api.stripe.com/v1/charges'))).toBe(true)
  })
})
