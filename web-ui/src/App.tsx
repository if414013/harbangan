import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { Layout } from './components/Layout'
import { AuthGate } from './components/AuthGate'
import { ToastProvider } from './components/Toast'
import { Dashboard } from './pages/Dashboard'
import { Config } from './pages/Config'

export default function App() {
  return (
    <ToastProvider>
      <AuthGate>
        <BrowserRouter basename="/_ui">
          <Routes>
            <Route element={<Layout />}>
              <Route index element={<Dashboard />} />
              <Route path="config" element={<Config />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </AuthGate>
    </ToastProvider>
  )
}
