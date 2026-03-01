interface ErrorsPanelProps {
  errors: Record<string, number>
}

export function ErrorsPanel({ errors }: ErrorsPanelProps) {
  const keys = Object.keys(errors)

  if (keys.length === 0) {
    return <div className="empty-state">No errors recorded</div>
  }

  return (
    <div className="errors-grid">
      {keys.map(key => (
        <div key={key} className="error-chip">
          <div className="error-chip-count">{errors[key]}</div>
          <div className="error-chip-label">{key}</div>
        </div>
      ))}
    </div>
  )
}
