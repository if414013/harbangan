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
          <th>MODEL</th>
          <th>REQ</th>
          <th>AVG LAT</th>
          <th>IN TOK</th>
          <th>OUT TOK</th>
        </tr>
      </thead>
      <tbody>
        {models.length === 0 ? (
          <tr>
            <td colSpan={5} style={{ textAlign: 'center', color: 'var(--text-tertiary)', padding: 20 }}>
              {'> waiting for data'}
              <span className="cursor" />
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
