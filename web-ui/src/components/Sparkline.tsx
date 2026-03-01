import { useRef, useEffect } from 'react'

interface SparklineProps {
  data: number[]
}

export function Sparkline({ data }: SparklineProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || data.length < 2) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const w = canvas.offsetWidth
    const h = canvas.offsetHeight
    canvas.width = w * dpr
    canvas.height = h * dpr
    ctx.scale(dpr, dpr)
    ctx.clearRect(0, 0, w, h)

    const max = Math.max(...data) || 1
    const step = w / (data.length - 1)

    // Grid lines
    ctx.strokeStyle = 'rgba(255,255,255,0.04)'
    ctx.lineWidth = 1
    for (let g = 1; g < 4; g++) {
      const gy = (h / 4) * g
      ctx.beginPath()
      ctx.moveTo(0, gy)
      ctx.lineTo(w, gy)
      ctx.stroke()
    }

    // Line
    ctx.beginPath()
    for (let i = 0; i < data.length; i++) {
      const x = i * step
      const y = h - (data[i] / max) * (h - 10) - 5
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.strokeStyle = '#e8a230'
    ctx.lineWidth = 2
    ctx.lineJoin = 'round'
    ctx.stroke()

    // Gradient fill
    ctx.lineTo((data.length - 1) * step, h)
    ctx.lineTo(0, h)
    ctx.closePath()
    const grad = ctx.createLinearGradient(0, 0, 0, h)
    grad.addColorStop(0, 'rgba(232,162,48,0.2)')
    grad.addColorStop(1, 'rgba(232,162,48,0)')
    ctx.fillStyle = grad
    ctx.fill()

    // Endpoint dot
    const lastX = (data.length - 1) * step
    const lastY = h - (data[data.length - 1] / max) * (h - 10) - 5
    ctx.beginPath()
    ctx.arc(lastX, lastY, 3, 0, Math.PI * 2)
    ctx.fillStyle = '#e8a230'
    ctx.fill()
  }, [data])

  return <canvas ref={canvasRef} style={{ width: '100%', height: 100, display: 'block' }} />
}
