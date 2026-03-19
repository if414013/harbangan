import { useEffect, useState } from "react";
import { useSearchParams, useNavigate } from "react-router-dom";
import { FormField } from "../components/FormField";
import { loginWithPassword, verify2FA } from "../lib/api";
import type { StatusResponse } from "../lib/api";

const ERROR_MESSAGES: Record<string, string> = {
  domain_not_allowed:
    "Your email domain is not authorized. Contact your admin.",
  consent_denied: "Google sign-in was cancelled.",
  invalid_state: "Login session expired. Please try again.",
  email_not_verified: "Your Google email is not verified.",
  auth_failed: "Authentication failed. Please try again.",
};

type LoginState = "loading" | "login" | "2fa" | "redirect";

export function Login() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const [state, setState] = useState<LoginState>("loading");
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loginToken, setLoginToken] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [useRecoveryCode, setUseRecoveryCode] = useState(false);
  const [error, setError] = useState<string | null>(params.get("error"));
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    Promise.all([
      fetch("/_ui/api/auth/me", { credentials: "include" })
        .then((res) => (res.ok ? ("authenticated" as const) : null))
        .catch(() => null),
      fetch("/_ui/api/status", { credentials: "include" })
        .then((res) =>
          res.ok ? (res.json() as Promise<StatusResponse>) : null,
        )
        .catch(() => null),
    ]).then(([authResult, statusData]) => {
      if (authResult === "authenticated") {
        navigate("/", { replace: true });
        return;
      }
      setStatus(statusData);
      setState("login");
    });
  }, [navigate]);

  async function handlePasswordSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const res = await loginWithPassword(email, password);
      if (res.needs_2fa && res.login_token) {
        setLoginToken(res.login_token);
        setState("2fa");
      } else {
        setState("redirect");
        navigate("/", { replace: true });
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed");
    } finally {
      setSubmitting(false);
    }
  }

  async function handle2FASubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      await verify2FA(loginToken, totpCode);
      setState("redirect");
      navigate("/", { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "2FA verification failed");
    } finally {
      setSubmitting(false);
    }
  }

  function handleGoogleLogin() {
    window.location.href = "/_ui/api/auth/google";
  }

  if (state === "loading" || state === "redirect") {
    return (
      <div className="auth-overlay">
        <div
          role="status"
          aria-label="Loading"
          style={{
            color: "var(--text-tertiary)",
            fontSize: "0.8rem",
            fontFamily: "var(--font-mono)",
          }}
        >
          Loading...
        </div>
      </div>
    );
  }

  if (state === "2fa") {
    return (
      <div className="auth-overlay">
        <div className="auth-card">
          <div className="auth-logo">
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="var(--bg)"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M6 2v20" />
              <path d="M18 2v20" />
              <path d="M6 2h12" />
              <path d="M6 22h12" />
              <path d="M12 8l2.5 2.5-2.5 2.5-2.5-2.5z" />
            </svg>
          </div>
          <h2>
            <span aria-hidden="true">{"> "}</span>2FA VERIFICATION
            <span className="cursor" aria-hidden="true" />
          </h2>
          <p>
            {useRecoveryCode
              ? "enter your recovery code"
              : "enter your 6-digit code"}
          </p>

          {error && (
            <div className="login-error" role="alert" aria-live="assertive">
              {error}
            </div>
          )}

          <form onSubmit={handle2FASubmit}>
            {useRecoveryCode ? (
              <FormField
                label="Recovery code"
                hint="Enter one of your 32-character recovery codes"
              >
                <input
                  className="auth-input"
                  type="text"
                  inputMode="text"
                  autoComplete="one-time-code"
                  placeholder="recovery code"
                  maxLength={32}
                  value={totpCode}
                  onChange={(e) => setTotpCode(e.target.value)}
                  autoFocus
                  required
                />
              </FormField>
            ) : (
              <FormField
                label="Authentication code"
                hint="Enter the 6-digit code from your authenticator app"
              >
                <input
                  className="auth-input totp-input auth-2fa-input"
                  type="text"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  placeholder="000000"
                  maxLength={6}
                  value={totpCode}
                  onChange={(e) => setTotpCode(e.target.value)}
                  autoFocus
                  required
                />
              </FormField>
            )}
            <button className="auth-submit" type="submit" disabled={submitting}>
              {submitting ? "$ verifying..." : "$ verify"}
            </button>
          </form>

          <button
            type="button"
            className="auth-toggle-link"
            onClick={() => {
              setUseRecoveryCode(!useRecoveryCode);
              setTotpCode("");
              setError(null);
            }}
          >
            {useRecoveryCode
              ? "use authenticator code instead"
              : "use recovery code instead"}
          </button>
        </div>
      </div>
    );
  }

  const googleEnabled = status?.auth_google_enabled ?? false;
  const passwordEnabled = status?.auth_password_enabled ?? false;

  return (
    <div className="auth-overlay">
      <div className="auth-card">
        <div className="auth-logo">
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="var(--bg)"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M6 2v20" />
            <path d="M18 2v20" />
            <path d="M6 2h12" />
            <path d="M6 22h12" />
            <path d="M12 8l2.5 2.5-2.5 2.5-2.5-2.5z" />
          </svg>
        </div>
        <h2>
          <span aria-hidden="true">{"> "}</span>HARBANGAN
          <span className="cursor" aria-hidden="true" />
        </h2>
        <p>Sign in to manage your API gateway</p>

        {error && (
          <div className="login-error" role="alert" aria-live="assertive">
            {ERROR_MESSAGES[error] || error}
          </div>
        )}

        {passwordEnabled && (
          <form onSubmit={handlePasswordSubmit}>
            <FormField label="Email address" hint="Your account email">
              <input
                className="auth-input"
                type="email"
                placeholder="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                autoComplete="email"
                required
                autoFocus
              />
            </FormField>
            <FormField label="Password">
              <input
                className="auth-input"
                type="password"
                placeholder="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
                required
              />
            </FormField>
            <p className="auth-helper-text">
              Forgot password? Contact your administrator.
            </p>
            <button className="auth-submit" type="submit" disabled={submitting}>
              {submitting ? "$ signing in..." : "$ sign in"}
            </button>
          </form>
        )}

        {passwordEnabled && googleEnabled && (
          <div className="auth-divider">
            <span className="auth-divider-line" />
            <span className="auth-divider-text">or</span>
            <span className="auth-divider-line" />
          </div>
        )}

        {googleEnabled && (
          <button
            className="auth-submit"
            type="button"
            onClick={handleGoogleLogin}
          >
            $ sign in with google
          </button>
        )}

        {!passwordEnabled && !googleEnabled && (
          <div className="login-error" role="alert">
            No authentication methods are enabled. Contact your administrator.
          </div>
        )}
      </div>
    </div>
  );
}
