import { useState, useEffect, useCallback, useRef } from 'react'
import { pollDeviceCode } from '../lib/api'

interface DeviceCodeDisplayProps {
  userCode: string
  verificationUri: string
  verificationUriComplete: string
  deviceCodeId: string
  onComplete: () => void
  onError: (message: string) => void
  onCancel: () => void
}

export function DeviceCodeDisplay({
  userCode,
  verificationUri,
  verificationUriComplete,
  deviceCodeId,
  onComplete,
  onError,
  onCancel,
}: DeviceCodeDisplayProps) {
  const [copied, setCopied] = useState(false)
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pollIntervalRef = useRef(5)
  const mountedRef = useRef(true)

  const stopPolling = useCallback(() => {
    if (pollRef.current) clearTimeout(pollRef.current)
  }, [])

  const poll = useCallback(async () => {
    if (!mountedRef.current) return
    try {
      const result = await pollDeviceCode(deviceCodeId)
      if (!mountedRef.current) return
      if (result.status === 'complete') {
        stopPolling()
        onComplete()
      } else if (result.status === 'slow_down') {
        pollIntervalRef.current = 10
        pollRef.current = setTimeout(poll, pollIntervalRef.current * 1000)
      } else {
        pollRef.current = setTimeout(poll, pollIntervalRef.current * 1000)
      }
    } catch (err) {
      if (!mountedRef.current) return
      stopPolling()
      onError(err instanceof Error ? err.message : 'Polling failed')
    }
  }, [deviceCodeId, stopPolling, onComplete, onError])

  useEffect(() => {
    mountedRef.current = true
    pollRef.current = setTimeout(poll, pollIntervalRef.current * 1000)
    return () => {
      mountedRef.current = false
      stopPolling()
    }
  }, [poll, stopPolling])

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(userCode)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // Fallback: ignore
    }
  }

  return (
    <div className="device-code-wrap">
      <p className="device-code-hint">enter this code when prompted</p>

      <button
        type="button"
        onClick={copyCode}
        className="device-code-btn"
        aria-label={`Copy code ${userCode}`}
      >
        <div className="device-code-value">{userCode}</div>
        <div className="device-code-copy">
          {copied ? '[copied]' : '[click to copy]'}
        </div>
      </button>

      <a
        href={verificationUriComplete}
        target="_blank"
        rel="noopener noreferrer"
        className="device-code-link"
      >
        [open] verification page
      </a>
      <span className="device-code-uri">{verificationUri}</span>

      <div className="device-code-polling">
        <span className="cursor" />
        polling...
      </div>

      <button
        type="button"
        onClick={() => { stopPolling(); onCancel() }}
        className="device-code-cancel"
      >
        $ cancel
      </button>
    </div>
  )
}
