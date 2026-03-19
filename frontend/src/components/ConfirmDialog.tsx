import { useEffect, useRef } from "react";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel?: string;
  variant?: "danger" | "warning" | "default";
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  variant = "default",
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    confirmRef.current?.focus();
  }, []);

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onCancel]);

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div
        className="modal-box confirm-dialog"
        onClick={(e) => e.stopPropagation()}
        role="alertdialog"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-message"
      >
        <h3 id="confirm-title" className="confirm-dialog-title">
          {variant === "danger" && (
            <span className="confirm-dialog-icon" aria-hidden="true">
              &#9888;
            </span>
          )}
          {title}
        </h3>
        <p id="confirm-message" className="confirm-dialog-message">
          {message}
        </p>
        <div className="modal-actions">
          <button className="btn-modal-cancel" onClick={onCancel}>
            Cancel
          </button>
          <button
            ref={confirmRef}
            className={`btn-modal-confirm ${variant === "danger" ? "btn-danger" : variant === "warning" ? "btn-warning" : ""}`}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
