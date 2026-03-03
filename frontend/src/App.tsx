import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { Layout } from './components/Layout'
import { SessionGate } from './components/SessionGate'
import { AdminGuard } from './components/AdminGuard'
import { ToastProvider } from './components/Toast'
import { Dashboard } from './pages/Dashboard'
import { Config } from './pages/Config'
import { Login } from './pages/Login'
import { Profile } from './pages/Profile'
import { Admin } from './pages/Admin'

export default function App() {
  return (
    <ToastProvider>
      <BrowserRouter basename="/_ui">
        <Routes>
          <Route path="login" element={<Login />} />
          <Route element={<SessionGate><Layout /></SessionGate>}>
            <Route index element={<Dashboard />} />
            <Route path="config" element={<AdminGuard><Config /></AdminGuard>} />
            <Route path="profile" element={<Profile />} />
            <Route path="admin" element={<AdminGuard><Admin /></AdminGuard>} />
          </Route>
        </Routes>
      </BrowserRouter>
    </ToastProvider>
  )
}
