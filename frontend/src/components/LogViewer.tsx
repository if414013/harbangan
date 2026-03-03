import { useState, useRef, useEffect, useCallback } from 'react'
import { useSSE } from '../lib/useSSE'

interface LogEntry {
  timestamp: string
  level: string
  message: string
}

const LEVELS = ['ALL', 'INFO', 'WARN', 'ERROR'] as const
const LEVEL_LABELS: Record<string, string> = {
  ALL: '[ALL]',
  INFO: '[INFO]',
  WARN: '[WARN]',
  ERROR: '[ERR]',
}

export function LogViewer() {
  const [logs, setLogs] = useState<LogEntry[]>([])
  const [filter, setFilter] = useState<string>('ALL')
  const [search, setSearch] = useState('')
  const [autoScroll, setAutoScroll] = useState(true)
  const containerRef = useRef<HTMLDivElement>(null)

  const handleLog = useCallback((data: unknown) => {
    const entries = Array.isArray(data) ? data as LogEntry[] : [data as LogEntry]
    setLogs(prev => {
      const next = [...prev, ...entries]
      return next.length > 500 ? next.slice(next.length - 500) : next
    })
  }, [])

  useSSE('/stream/logs', 'log', handleLog)

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight
    }
  }, [logs, autoScroll])

  const q = search.toLowerCase()
  const visible = logs.filter(entry => {
    if (filter !== 'ALL' && entry.level !== filter) return false
    if (q && !entry.message.toLowerCase().includes(q) && !entry.timestamp.toLowerCase().includes(q)) return false
    return true
  })

  return (
    <>
      <div className="log-toolbar">
        <div className="prompt-input-wrap">
          <input
            type="text"
            className="log-search"
            placeholder="grep..."
            value={search}
            onChange={e => setSearch(e.target.value)}
            aria-label="Search logs"
          />
        </div>
        <div className="pill-group">
          {LEVELS.map(level => (
            <button
              key={level}
              className={`pill${filter === level ? ' active' : ''}`}
              onClick={() => setFilter(level)}
            >
              {LEVEL_LABELS[level]}
            </button>
          ))}
        </div>
        <button
          className={`log-toggle${autoScroll ? ' on' : ''}`}
          onClick={() => setAutoScroll(v => !v)}
        >
          [tail -f]
        </button>
      </div>
      <div className="log-stream" ref={containerRef} role="log" aria-live="polite">
        {visible.map((entry, i) => (
          <div key={i} className={`log-line level-${entry.level}`}>
            <span className="log-line-prompt">$</span>
            {entry.timestamp} [{entry.level}] {entry.message}
          </div>
        ))}
      </div>
    </>
  )
}
