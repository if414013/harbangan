import { useState, useEffect } from 'react'
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { Layout } from './components/Layout'
import { AuthGate } from './components/AuthGate'
import { ToastProvider } from './components/Toast'
import { Dashboard } from './pages/Dashboard'
import { Config } from './pages/Config'
import { Setup } from './pages/Setup'
import { checkSetupStatus } from './lib/api'

export default function App() {
  const [setupComplete, setSetupComplete] = useState<boolean | null>(null)

  useEffect(() => {
    checkSetupStatus().then(setSetupComplete)
  }, [])

  if (setupComplete === null) {
    return (
      <div className="auth-overlay">
        <div style={{ color: 'var(--text-tertiary)', fontSize: '0.8rem', fontFamily: 'var(--font-mono)' }}>
          Loading...
        </div>
      </div>
    )
  }

  return (
    <ToastProvider>
      <BrowserRouter basename="/_ui">
        <Routes>
          <Route
            path="setup"
            element={
              setupComplete
                ? <Navigate to="/" replace />
                : <Setup onComplete={() => setSetupComplete(true)} />
            }
          />
          <Route
            element={
              setupComplete
                ? <AuthGate><Layout /></AuthGate>
                : <Navigate to="/setup" replace />
            }
          >
            <Route index element={<Dashboard />} />
            <Route path="config" element={<Config />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </ToastProvider>
  )
}
