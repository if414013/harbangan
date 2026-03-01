import { formatNum } from '../lib/format'

interface Model {
  name: string
  requests: number
  avg_latency_ms: number
  input_tokens: number
  output_tokens: number
}

interface ModelTableProps {
  models: Model[]
}

export function ModelTable({ models }: ModelTableProps) {
  return (
    <table className="data-table">
      <thead>
        <tr>
          <th>Model</th>
          <th>Requests</th>
          <th>Avg Latency</th>
          <th>Input Tokens</th>
          <th>Output Tokens</th>
        </tr>
      </thead>
      <tbody>
        {models.length === 0 ? (
          <tr>
            <td colSpan={5} style={{ textAlign: 'center', color: 'var(--text-tertiary)', padding: 24 }}>
              Waiting for data&hellip;
            </td>
          </tr>
        ) : (
          models.map(m => (
            <tr key={m.name}>
              <td>{m.name}</td>
              <td>{formatNum(m.requests)}</td>
              <td>{m.avg_latency_ms} ms</td>
              <td>{formatNum(m.input_tokens)}</td>
              <td>{formatNum(m.output_tokens)}</td>
            </tr>
          ))
        )}
      </tbody>
    </table>
  )
}
