import { useState, useEffect } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { Sidebar } from "./Sidebar";

function formatUptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function Layout() {
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [uptime, setUptime] = useState(0);
  const location = useLocation();

  useEffect(() => {
    const id = setInterval(() => setUptime((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    const denseRoutes = ["/config", "/usage", "/admin", "/providers"];
    const isDense = denseRoutes.some((r) => location.pathname.includes(r));
    if (isDense) {
      document.body.dataset.dense = "true";
    } else {
      delete document.body.dataset.dense;
    }
  }, [location.pathname]);

  useEffect(() => {
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape" && sidebarOpen) setSidebarOpen(false);
    };
    document.addEventListener("keydown", handleEsc);
    return () => document.removeEventListener("keydown", handleEsc);
  }, [sidebarOpen]);

  const pageTitle = (() => {
    if (location.pathname.includes("/config")) return "configuration";
    if (location.pathname.includes("/guardrails")) return "guardrails";
    if (location.pathname.includes("/providers")) return "providers";
    if (location.pathname.includes("/usage")) return "usage";
    if (location.pathname.includes("/profile")) return "profile";
    if (location.pathname.includes("/admin")) return "administration";
    return "dashboard";
  })();

  return (
    <div className="shell">
      <a href="#main-content" className="skip-to-content">
        Skip to content
      </a>
      {sidebarOpen && (
        <div
          className="sidebar-backdrop"
          onClick={() => setSidebarOpen(false)}
        />
      )}
      <Sidebar open={sidebarOpen} onClose={() => setSidebarOpen(false)} />
      <header className="top-bar">
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <button
            className="hamburger"
            onClick={() => setSidebarOpen((v) => !v)}
            aria-label="Toggle navigation"
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            >
              <line x1="3" y1="6" x2="21" y2="6" />
              <line x1="3" y1="12" x2="21" y2="12" />
              <line x1="3" y1="18" x2="21" y2="18" />
            </svg>
          </button>
          <span className="page-title">
            <span aria-hidden="true">{"> "}</span>
            {pageTitle}
            <span className="cursor" aria-hidden="true" />
          </span>
        </div>
        <div className="top-bar-info">
          <span>up {formatUptime(uptime)}</span>
          <span>v1.0.8</span>
        </div>
      </header>
      <main className="main" id="main-content">
        <Outlet />
      </main>
    </div>
  );
}
