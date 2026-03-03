interface MetricCardProps {
  label: string
  badge: string
  value: string | number
  percent: number
}

function blockBar(percent: number, width = 20): string {
  const clamped = Math.min(100, Math.max(0, percent))
  const filled = Math.round((clamped / 100) * width)
  return '\u2588'.repeat(filled) + '\u2591'.repeat(width - filled)
}

export function MetricCard({ label, badge, value, percent }: MetricCardProps) {
  return (
    <div className="metric-card">
      <div className="metric-header">
        <span className="metric-label">{label}</span>
        <span className="metric-badge">[{badge}]</span>
      </div>
      <div className="metric-value">
        {typeof value === 'number' && value % 1 !== 0 ? value.toFixed(1) : value}
      </div>
      <div className="metric-bar-blocks">
        {blockBar(percent)}
        <span className="bar-percent">{Math.round(percent)}%</span>
      </div>
    </div>
  )
}
