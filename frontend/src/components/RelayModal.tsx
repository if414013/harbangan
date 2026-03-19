import { useState, useEffect, useRef } from "react";
import { getProvidersStatus } from "../lib/api";

const RELAY_TIMEOUT_MS = 10 * 60 * 1000;

interface RelayModalProps {
  provider: string;
  relayScriptUrl: string;
  onConnected: () => void;
  onClose: () => void;
}

export function RelayModal({
  provider,
  relayScriptUrl,
  onConnected,
  onClose,
}: RelayModalProps) {
  const [copied, setCopied] = useState(false);
  const [timedOut, setTimedOut] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);

  const curlCommand = `curl -fsSL '${relayScriptUrl}' | sh`;

  useEffect(() => {
    mountedRef.current = true;

    pollRef.current = setInterval(async () => {
      if (!mountedRef.current) return;
      try {
        const status = await getProvidersStatus();
        if (!mountedRef.current) return;
        const p = status.providers[provider];
        if (p?.connected) {
          onConnected();
        }
      } catch {
        // ignore poll errors
      }
    }, 2000);

    timeoutRef.current = setTimeout(() => {
      if (!mountedRef.current) return;
      setTimedOut(true);
      if (pollRef.current) clearInterval(pollRef.current);
    }, RELAY_TIMEOUT_MS);

    return () => {
      mountedRef.current = false;
      if (pollRef.current) clearInterval(pollRef.current);
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, [provider, onConnected]);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(curlCommand);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-box relay-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <h3>connect {provider}</h3>
        {timedOut ? (
          <>
            <p className="relay-timeout">
              Connection timed out. Click connect to try again.
            </p>
            <div className="modal-actions">
              <button type="button" onClick={onClose}>
                $ close
              </button>
            </div>
          </>
        ) : (
          <>
            <p>Run this in your terminal:</p>
            <div className="relay-command-wrap">
              <code className="relay-command">{curlCommand}</code>
              <button
                type="button"
                className="relay-copy-btn"
                onClick={handleCopy}
              >
                {copied ? "[copied]" : "[copy]"}
              </button>
            </div>
            <div className="device-code-polling">
              <span className="cursor" />
              waiting for authorization...
            </div>
            <div className="modal-actions">
              <button type="button" onClick={onClose}>
                $ cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
