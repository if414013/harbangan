import { useState, useCallback } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useSSE } from '../lib/useSSE'
import { MetricCard } from '../components/MetricCard'
import { Sparkline } from '../components/Sparkline'
import { ModelTable } from '../components/ModelTable'
import { ErrorsPanel } from '../components/ErrorsPanel'
import { LogViewer } from '../components/LogViewer'

interface MetricsData {
  active_connections?: number
  max_connections?: number
  cpu_percent?: number
  memory_mb?: number
  max_memory_mb?: number
  request_rate?: number
  latency?: { p50: number; p95: number; p99: number }
  models?: Array<{
    name: string
    requests: number
    avg_latency_ms: number
    input_tokens: number
    output_tokens: number
  }>
  errors?: Record<string, number>
}

interface LayoutContext {
  setConnected: (v: boolean) => void
}

export function Dashboard() {
  const { setConnected } = useOutletContext<LayoutContext>()
  const [connections, setConnections] = useState(0)
  const [maxConnections, setMaxConnections] = useState(100)
  const [cpu, setCpu] = useState(0)
  const [memory, setMemory] = useState(0)
  const [maxMemory, setMaxMemory] = useState(1024)
  const [sparkData, setSparkData] = useState<number[]>([])
  const [latency, setLatency] = useState({ p50: 0, p95: 0, p99: 0 })
  const [models, setModels] = useState<MetricsData['models']>([])
  const [errors, setErrors] = useState<Record<string, number>>({})

  const handleMetrics = useCallback((raw: unknown) => {
    const data = raw as MetricsData
    if (data.active_connections !== undefined) setConnections(data.active_connections)
    if (data.max_connections !== undefined) setMaxConnections(data.max_connections)
    if (data.cpu_percent !== undefined) setCpu(data.cpu_percent)
    if (data.memory_mb !== undefined) setMemory(data.memory_mb)
    if (data.max_memory_mb !== undefined) setMaxMemory(data.max_memory_mb)
    if (data.request_rate !== undefined) {
      setSparkData(prev => {
        const next = [...prev, data.request_rate!]
        return next.length > 60 ? next.slice(next.length - 60) : next
      })
    }
    if (data.latency) setLatency(data.latency)
    if (data.models) setModels(data.models)
    if (data.errors) setErrors(data.errors)
  }, [])

  useSSE('/stream/metrics', 'metrics', handleMetrics, setConnected)

  return (
    <>
      <div className="metrics-grid">
        <MetricCard
          label="Active Connections"
          badge="live"
          value={connections}
          percent={(connections / maxConnections) * 100}
        />
        <MetricCard
          label="CPU Usage"
          badge="%"
          value={cpu}
          percent={cpu}
        />
        <MetricCard
          label="Memory"
          badge="MB"
          value={memory}
          percent={(memory / maxMemory) * 100}
        />
      </div>

      <div className="two-col">
        <div className="card">
          <div className="card-header">
            <span className="card-title">Request Rate</span>
            <span className="card-subtitle">req/s</span>
          </div>
          <Sparkline data={sparkData} />
        </div>
        <div className="card">
          <div className="card-header">
            <span className="card-title">Latency Percentiles</span>
          </div>
          <div className="latency-grid">
            <div className="latency-cell">
              <div className="latency-percentile">p50</div>
              <div className="latency-value">{latency.p50 ? `${latency.p50} ms` : '\u2014'}</div>
            </div>
            <div className="latency-cell">
              <div className="latency-percentile">p95</div>
              <div className="latency-value">{latency.p95 ? `${latency.p95} ms` : '\u2014'}</div>
            </div>
            <div className="latency-cell">
              <div className="latency-percentile">p99</div>
              <div className="latency-value">{latency.p99 ? `${latency.p99} ms` : '\u2014'}</div>
            </div>
          </div>
        </div>
      </div>

      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">Model Stats</span>
        </div>
        <ModelTable models={models || []} />
      </div>

      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">Errors</span>
        </div>
        <ErrorsPanel errors={errors} />
      </div>

      <div className="card">
        <div className="card-header">
          <span className="card-title">Live Logs</span>
        </div>
        <LogViewer />
      </div>
    </>
  )
}
