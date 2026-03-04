import { datadogRum } from '@datadog/browser-rum'

const clientToken = import.meta.env.VITE_DD_CLIENT_TOKEN as string | undefined
const applicationId = import.meta.env.VITE_DD_APPLICATION_ID as string | undefined
const env = (import.meta.env.VITE_DD_ENV as string | undefined) ?? 'production'
const service = (import.meta.env.VITE_DD_SERVICE as string | undefined) ?? 'rkgw-frontend'

export function initDatadog(): void {
  if (!clientToken || !applicationId) {
    return
  }

  datadogRum.init({
    applicationId,
    clientToken,
    site: 'datadoghq.com',
    service,
    env,
    sessionSampleRate: 100,
    sessionReplaySampleRate: 0,
    trackUserInteractions: true,
    trackResources: true,
    trackLongTasks: true,
    allowedTracingUrls: [
      (url) => url.includes('/_ui/api'),
      (url) => url.includes('/v1/'),
    ],
  })
}
