import { UserTable } from '../components/UserTable'
import { DomainManager } from '../components/DomainManager'
import { useSession } from '../components/SessionGate'

export function Admin() {
  const { setupComplete } = useSession()

  return (
    <>
      {!setupComplete && (
        <div className="setup-banner">
          <div className="setup-banner-icon">!</div>
          <div>
            <strong>Welcome, admin!</strong> Your gateway is almost ready.
            Add your organization's domain below to restrict who can sign in.
            Leave empty to allow any Google account.
          </div>
        </div>
      )}

      <h2 className="section-header">DOMAIN ALLOWLIST</h2>
      <div className="mb-24">
        <DomainManager />
      </div>

      <h2 className="section-header">USER MANAGEMENT</h2>
      <UserTable />
    </>
  )
}
