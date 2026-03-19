import type { ReactNode } from "react";

interface FormFieldProps {
  label: string;
  hint?: string;
  error?: string;
  required?: boolean;
  htmlFor?: string;
  children: ReactNode;
}

export function FormField({
  label,
  hint,
  error,
  required,
  htmlFor,
  children,
}: FormFieldProps) {
  return (
    <div className={`form-field${error ? " form-field-error" : ""}`}>
      <label className="form-field-label" htmlFor={htmlFor}>
        {label}
        {required && (
          <span className="form-field-required" aria-label="required">
            *
          </span>
        )}
      </label>
      {children}
      {hint && !error && <p className="form-field-hint">{hint}</p>}
      {error && (
        <p className="form-field-error-text" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
