import { datadogRum } from '@datadog/browser-rum'

const clientToken = import.meta.env.VITE_DD_CLIENT_TOKEN as string | undefined
const applicationId = import.meta.env.VITE_DD_APPLICATION_ID as string | undefined
const env = (import.meta.env.VITE_DD_ENV as string | undefined) ?? 'production'
const service = (import.meta.env.VITE_DD_SERVICE as string | undefined) ?? 'rkgw-frontend'
// VITE_DD_SITE allows EU/Gov deployments to route RUM data to the correct region
// (e.g. datadoghq.eu, ap1.datadoghq.com). Defaults to US1.
const site = (import.meta.env.VITE_DD_SITE as string | undefined) ?? 'datadoghq.com'

export function initDatadog(): void {
  if (!clientToken || !applicationId) {
    return
  }

  // trackingConsent: this gateway is internal-only enterprise tooling.
  // Operators deploying to end-users subject to GDPR should set
  // trackingConsent: 'not-granted' and upgrade to 'granted' after user consent.
  datadogRum.init({
    applicationId,
    clientToken,
    site,
    service,
    env,
    sessionSampleRate: 100,
    sessionReplaySampleRate: 0, // Disabled: session replay would capture sensitive UI content (API keys, tokens)
    trackUserInteractions: true,
    trackResources: true,
    trackLongTasks: true,
    allowedTracingUrls: [
      // Origin-anchored to prevent trace header injection into third-party APIs
      (url) => url.startsWith(window.location.origin + '/_ui/api'),
      (url) => url.startsWith(window.location.origin + '/v1/'),
    ],
  })
}
