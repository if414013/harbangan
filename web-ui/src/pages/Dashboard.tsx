import { useReducer, useCallback } from 'react'
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

interface MetricsState {
  connections: number
  maxConnections: number
  cpu: number
  memory: number
  maxMemory: number
  sparkData: number[]
  latency: { p50: number; p95: number; p99: number }
  models: MetricsData['models']
  errors: Record<string, number>
}

const initialState: MetricsState = {
  connections: 0,
  maxConnections: 100,
  cpu: 0,
  memory: 0,
  maxMemory: 1024,
  sparkData: [],
  latency: { p50: 0, p95: 0, p99: 0 },
  models: [],
  errors: {},
}

function metricsReducer(state: MetricsState, data: MetricsData): MetricsState {
  const next = { ...state }
  if (data.active_connections !== undefined) next.connections = data.active_connections
  if (data.max_connections !== undefined) next.maxConnections = data.max_connections
  if (data.cpu_percent !== undefined) next.cpu = data.cpu_percent
  if (data.memory_mb !== undefined) next.memory = data.memory_mb
  if (data.max_memory_mb !== undefined) next.maxMemory = data.max_memory_mb
  if (data.request_rate !== undefined) {
    const updated = [...state.sparkData, data.request_rate]
    next.sparkData = updated.length > 60 ? updated.slice(updated.length - 60) : updated
  }
  if (data.latency) next.latency = data.latency
  if (data.models) next.models = data.models
  if (data.errors) next.errors = data.errors
  return next
}

interface LayoutContext {
  setConnected: (v: boolean) => void
}

export function Dashboard() {
  const { setConnected } = useOutletContext<LayoutContext>()
  const [state, dispatch] = useReducer(metricsReducer, initialState)

  const handleMetrics = useCallback((raw: unknown) => {
    dispatch(raw as MetricsData)
  }, [])

  useSSE('/stream/metrics', 'metrics', handleMetrics, setConnected)

  return (
    <>
      <div className="metrics-grid">
        <MetricCard
          label="Active Connections"
          badge="live"
          value={state.connections}
          percent={(state.connections / state.maxConnections) * 100}
        />
        <MetricCard
          label="CPU Usage"
          badge="%"
          value={state.cpu}
          percent={state.cpu}
        />
        <MetricCard
          label="Memory"
          badge="MB"
          value={state.memory}
          percent={(state.memory / state.maxMemory) * 100}
        />
      </div>

      <div className="two-col">
        <div className="card">
          <div className="card-header">
            <span className="card-title">Request Rate</span>
            <span className="card-subtitle">req/s</span>
          </div>
          <Sparkline data={state.sparkData} />
        </div>
        <div className="card">
          <div className="card-header">
            <span className="card-title">Latency Percentiles</span>
          </div>
          <div className="latency-grid">
            <div className="latency-cell">
              <div className="latency-percentile">p50</div>
              <div className="latency-value">{state.latency.p50 ? `${state.latency.p50} ms` : '\u2014'}</div>
            </div>
            <div className="latency-cell">
              <div className="latency-percentile">p95</div>
              <div className="latency-value">{state.latency.p95 ? `${state.latency.p95} ms` : '\u2014'}</div>
            </div>
            <div className="latency-cell">
              <div className="latency-percentile">p99</div>
              <div className="latency-value">{state.latency.p99 ? `${state.latency.p99} ms` : '\u2014'}</div>
            </div>
          </div>
        </div>
      </div>

      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">Model Stats</span>
        </div>
        <ModelTable models={state.models || []} />
      </div>

      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">Errors</span>
        </div>
        <ErrorsPanel errors={state.errors} />
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
