interface MetricCardProps {
  label: string
  badge: string
  value: string | number
  percent: number
}

export function MetricCard({ label, badge, value, percent }: MetricCardProps) {
  return (
    <div className="metric-card">
      <div className="metric-header">
        <span className="metric-label">{label}</span>
        <span className="metric-badge">{badge}</span>
      </div>
      <div className="metric-value">
        {typeof value === 'number' && value % 1 !== 0 ? value.toFixed(1) : value}
      </div>
      <div className="metric-bar-track">
        <div className="metric-bar-fill" style={{ width: `${Math.min(100, percent)}%` }} />
      </div>
    </div>
  )
}
