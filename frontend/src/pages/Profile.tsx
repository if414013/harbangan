import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { ApiKeyManager } from "../components/ApiKeyManager";
import { PageHeader } from "../components/PageHeader";
import { useSession } from "../components/SessionGate";
import { getStatus } from "../lib/api";

export function Profile() {
  const { user } = useSession();
  const navigate = useNavigate();
  const [passwordAuthEnabled, setPasswordAuthEnabled] = useState(false);
  const [googleAuthEnabled, setGoogleAuthEnabled] = useState(false);

  useEffect(() => {
    getStatus()
      .then((s) => {
        setPasswordAuthEnabled(s.auth_password_enabled);
        setGoogleAuthEnabled(s.auth_google_enabled);
      })
      .catch(() => {});
  }, []);

  return (
    <>
      <PageHeader
        title="profile"
        description="Manage your account, API keys, and security settings."
      />
      <div className="card mb-24">
        <div className="card-header">
          <span className="card-title">{"> "}Account</span>
          <span
            style={{
              fontSize: "0.55rem",
              fontFamily: "var(--font-mono)",
              padding: "1px 5px",
              borderRadius: "var(--radius-sm)",
              background:
                user.role === "admin" ? "var(--green-dim)" : "var(--blue-dim)",
              color: user.role === "admin" ? "var(--green)" : "var(--blue)",
              whiteSpace: "nowrap",
            }}
          >
            {user.role}
          </span>
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "4px 0",
          }}
        >
          {user.picture_url && (
            <img
              src={user.picture_url}
              alt=""
              style={{
                width: 32,
                height: 32,
                borderRadius: "var(--radius)",
                opacity: 0.85,
              }}
            />
          )}
          <div>
            <div
              style={{
                fontSize: "0.82rem",
                color: "var(--text)",
                fontWeight: 500,
              }}
            >
              {user.name}
            </div>
            <div style={{ fontSize: "0.72rem", color: "var(--text-tertiary)" }}>
              {user.email}
            </div>
          </div>
        </div>
      </div>

      <h2 className="section-header">API KEYS</h2>
      <div className="mb-24">
        <ApiKeyManager />
      </div>

      {(passwordAuthEnabled || googleAuthEnabled) && (
        <>
          <h2 className="section-header">SECURITY</h2>
          <div className="card">
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {googleAuthEnabled && (
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                  }}
                >
                  <span
                    style={{
                      fontSize: "0.72rem",
                      color: "var(--text-secondary)",
                    }}
                  >
                    Google:{" "}
                    {user.google_linked ? (
                      <span style={{ color: "var(--green)" }}>LINKED</span>
                    ) : (
                      <span style={{ color: "var(--text-tertiary)" }}>
                        NOT LINKED
                      </span>
                    )}
                  </span>
                  {!user.google_linked && (
                    <button
                      className="btn-save"
                      type="button"
                      onClick={() => {
                        window.location.href = "/_ui/api/auth/google/link";
                      }}
                    >
                      $ link google
                    </button>
                  )}
                </div>
              )}
              {passwordAuthEnabled && (
                <>
                  {user.auth_method === "password" ? (
                    <>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "space-between",
                        }}
                      >
                        <span
                          style={{
                            fontSize: "0.72rem",
                            color: "var(--text-secondary)",
                          }}
                        >
                          2FA:{" "}
                          {user.totp_enabled ? (
                            <span style={{ color: "var(--green)" }}>
                              ENABLED
                            </span>
                          ) : (
                            <span style={{ color: "var(--red)" }}>
                              NOT SET UP
                            </span>
                          )}
                        </span>
                      </div>
                      <div style={{ display: "flex", gap: 8 }}>
                        <button
                          className="btn-save"
                          type="button"
                          onClick={() => navigate("/change-password")}
                        >
                          $ change password
                        </button>
                        <button
                          className="btn-save"
                          type="button"
                          onClick={() => navigate("/setup-2fa")}
                        >
                          $ reset 2fa
                        </button>
                      </div>
                    </>
                  ) : (
                    <>
                      <span
                        style={{
                          fontSize: "0.72rem",
                          color: "var(--text-secondary)",
                        }}
                      >
                        set a password to enable 2FA and password login
                      </span>
                      <div>
                        <button
                          className="btn-save"
                          type="button"
                          onClick={() => navigate("/change-password")}
                        >
                          $ set password
                        </button>
                      </div>
                    </>
                  )}
                </>
              )}
            </div>
          </div>
        </>
      )}
    </>
  );
}
