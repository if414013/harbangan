import { useState, type FormEvent } from 'react'
import { getApiKey, setApiKey } from '../lib/auth'

export function AuthGate({ children }: { children: React.ReactNode }) {
  const [hasKey, setHasKey] = useState(() => !!getApiKey())
  const [input, setInput] = useState('')

  if (hasKey) return <>{children}</>

  function handleSubmit(e: FormEvent) {
    e.preventDefault()
    if (input.trim()) {
      setApiKey(input.trim())
      setHasKey(true)
    }
  }

  return (
    <div className="auth-overlay">
      <form className="auth-card" onSubmit={handleSubmit}>
        <div className="auth-logo">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#101014" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/>
          </svg>
        </div>
        <h2>Kiro Gateway</h2>
        <p>Enter your API key to continue</p>
        <input
          className="auth-input"
          type="password"
          placeholder="API key"
          autoComplete="current-password"
          value={input}
          onChange={e => setInput(e.target.value)}
          autoFocus
        />
        <button className="auth-submit" type="submit">Connect</button>
      </form>
    </div>
  )
}
