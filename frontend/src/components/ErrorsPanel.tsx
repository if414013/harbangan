interface ErrorsPanelProps {
  errors: Record<string, number>
}

export function ErrorsPanel({ errors }: ErrorsPanelProps) {
  const keys = Object.keys(errors)

  if (keys.length === 0) {
    return <div className="empty-state"><span className="tag-ok">NO ERRORS</span></div>
  }

  return (
    <div className="errors-list">
      {keys.map(key => (
        <div key={key} className="error-item">
          <span className="error-item-tag">[ERR]</span> {key}
          <span className="error-item-count">x{errors[key]}</span>
        </div>
      ))}
    </div>
  )
}
