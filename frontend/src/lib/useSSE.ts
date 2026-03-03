import { useEffect, useRef } from 'react'

export function useSSE(
  path: string,
  eventName: string,
  onMessage: (data: unknown) => void,
  onStatus?: (connected: boolean) => void,
) {
  const cbRef = useRef(onMessage)
  const statusRef = useRef(onStatus)

  useEffect(() => { cbRef.current = onMessage }, [onMessage])
  useEffect(() => { statusRef.current = onStatus }, [onStatus])

  useEffect(() => {
    let source: EventSource | null = null
    let timer: ReturnType<typeof setTimeout>
    let retryDelay = 1000
    let stopped = false

    function connect() {
      if (stopped) return
      source = new EventSource(`/_ui/api${path}`, { withCredentials: true })
      statusRef.current?.(true)
      retryDelay = 1000

      source.addEventListener(eventName, (e: MessageEvent) => {
        cbRef.current(JSON.parse(e.data))
      })

      source.onerror = () => {
        statusRef.current?.(false)
        source?.close()
        if (!stopped) {
          timer = setTimeout(connect, retryDelay)
          retryDelay = Math.min(retryDelay * 1.5, 30000)
        }
      }
    }

    connect()
    return () => {
      stopped = true
      source?.close()
      clearTimeout(timer)
    }
  }, [path, eventName])
}
