import { useEffect, useRef } from 'react'
import { getApiKey } from './auth'

export function useSSE(
  path: string,
  eventName: string,
  onMessage: (data: unknown) => void,
  onStatus?: (connected: boolean) => void,
) {
  const cbRef = useRef(onMessage)
  cbRef.current = onMessage
  const statusRef = useRef(onStatus)
  statusRef.current = onStatus

  useEffect(() => {
    let source: EventSource | null = null
    let timer: ReturnType<typeof setTimeout>
    let retryDelay = 1000

    function connect() {
      const key = getApiKey()
      if (!key) return
      source = new EventSource(`/_ui/api${path}?api_key=${encodeURIComponent(key)}`)
      statusRef.current?.(true)
      retryDelay = 1000

      source.addEventListener(eventName, (e: MessageEvent) => {
        cbRef.current(JSON.parse(e.data))
      })

      source.onerror = () => {
        statusRef.current?.(false)
        source?.close()
        timer = setTimeout(connect, retryDelay)
        retryDelay = Math.min(retryDelay * 1.5, 30000)
      }
    }

    connect()
    return () => {
      source?.close()
      clearTimeout(timer)
    }
  }, [path, eventName])
}
