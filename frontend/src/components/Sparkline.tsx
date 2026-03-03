import { useMemo } from 'react'

interface SparklineProps {
  data: number[]
}

export function Sparkline({ data }: SparklineProps) {
  if (data.length < 2) {
    return (
      <div className="spark-empty">
        {'> waiting for data'}
        <span className="cursor" />
      </div>
    )
  }

  const { path, areaPath, max, latest, points } = useMemo(() => {
    const max = Math.max(...data) || 1
    const w = 100
    const h = 40
    const pad = 1
    const latest = data[data.length - 1]

    const pts = data.map((v, i) => ({
      x: pad + (i / (data.length - 1)) * (w - pad * 2),
      y: pad + (1 - v / max) * (h - pad * 2),
    }))

    // Smooth curve through points using catmull-rom → cubic bezier
    let d = `M${pts[0].x},${pts[0].y}`
    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[Math.max(0, i - 1)]
      const p1 = pts[i]
      const p2 = pts[i + 1]
      const p3 = pts[Math.min(pts.length - 1, i + 2)]

      const cp1x = p1.x + (p2.x - p0.x) / 6
      const cp1y = p1.y + (p2.y - p0.y) / 6
      const cp2x = p2.x - (p3.x - p1.x) / 6
      const cp2y = p2.y - (p3.y - p1.y) / 6

      d += ` C${cp1x},${cp1y} ${cp2x},${cp2y} ${p2.x},${p2.y}`
    }

    const area = `${d} L${pts[pts.length - 1].x},${h} L${pts[0].x},${h} Z`

    return { path: d, areaPath: area, max, latest, points: pts }
  }, [data])

  const lastPt = points[points.length - 1]

  return (
    <div className="spark-wrap" title={`latest: ${latest}`}>
      <svg
        viewBox="0 0 100 40"
        preserveAspectRatio="none"
        className="spark-svg"
      >
        <defs>
          <linearGradient id="sparkGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--green)" stopOpacity="0.25" />
            <stop offset="100%" stopColor="var(--green)" stopOpacity="0" />
          </linearGradient>
          <filter id="sparkGlow">
            <feGaussianBlur stdDeviation="1.2" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* Grid lines */}
        {[0.25, 0.5, 0.75].map(f => (
          <line
            key={f}
            x1="0" y1={40 * f} x2="100" y2={40 * f}
            stroke="var(--border-subtle)"
            strokeWidth="0.3"
            strokeDasharray="1 2"
          />
        ))}

        {/* Area fill */}
        <path d={areaPath} fill="url(#sparkGrad)" />

        {/* Line */}
        <path
          d={path}
          fill="none"
          stroke="var(--green)"
          strokeWidth="1"
          strokeLinecap="round"
          strokeLinejoin="round"
          filter="url(#sparkGlow)"
          className="spark-line"
        />

        {/* Latest point pulse */}
        <circle cx={lastPt.x} cy={lastPt.y} r="2.5" fill="var(--green)" opacity="0.3" className="spark-pulse" />
        <circle cx={lastPt.x} cy={lastPt.y} r="1.2" fill="var(--green)" />
      </svg>

      <div className="spark-meta">
        <span className="spark-latest">{latest.toFixed(1)}</span>
        <span className="spark-max">peak {max.toFixed(1)}</span>
      </div>
    </div>
  )
}
