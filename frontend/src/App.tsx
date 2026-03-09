import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { Layout } from './components/Layout'
import { SessionGate } from './components/SessionGate'
import { AdminGuard } from './components/AdminGuard'
import { ToastProvider } from './components/Toast'
import { ThemeProvider } from './lib/theme'
import { Config } from './pages/Config'
import { Login } from './pages/Login'
import { Profile } from './pages/Profile'
import { Admin } from './pages/Admin'
import { UserDetail } from './pages/UserDetail'
import { Guardrails } from './pages/Guardrails'
import { McpClients } from './pages/McpClients'
import { Models } from './pages/Models'

export default function App() {
  return (
    <ThemeProvider>
    <ToastProvider>
      <BrowserRouter basename="/_ui">
        <Routes>
          <Route path="login" element={<Login />} />
          <Route element={<SessionGate><Layout /></SessionGate>}>
            <Route index element={<Navigate to="profile" replace />} />
            <Route path="config" element={<AdminGuard><Config /></AdminGuard>} />
            <Route path="profile" element={<Profile />} />
            <Route path="guardrails" element={<AdminGuard><Guardrails /></AdminGuard>} />
            <Route path="mcp" element={<AdminGuard><McpClients /></AdminGuard>} />
            <Route path="models" element={<AdminGuard><Models /></AdminGuard>} />
            <Route path="admin" element={<AdminGuard><Admin /></AdminGuard>} />
            <Route path="admin/users/:userId" element={<AdminGuard><UserDetail /></AdminGuard>} />
          </Route>
        </Routes>
      </BrowserRouter>
    </ToastProvider>
    </ThemeProvider>
  )
}
