import type { ReactNode } from "react";

interface PageHeaderProps {
  title: string;
  description?: string;
  actions?: ReactNode;
  badge?: ReactNode;
}

export function PageHeader({
  title,
  description,
  actions,
  badge,
}: PageHeaderProps) {
  return (
    <div className="page-header">
      <div className="page-header-content">
        <h1 className="page-header-title">
          <span className="page-header-prefix" aria-hidden="true">
            {"> "}
          </span>
          {title}
          {badge && <span className="page-header-badge">{badge}</span>}
        </h1>
        {description && <p className="page-header-desc">{description}</p>}
      </div>
      {actions && <div className="page-header-actions">{actions}</div>}
    </div>
  );
}
