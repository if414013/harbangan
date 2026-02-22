//! Backend router for deciding Kiro vs Antigravity (Cloud Code) per request.
//!
//! Inspects the requested model name and the antigravity configuration
//! to determine which backend should handle the request.

use super::models::is_antigravity_model;
use crate::config::AntigravityConfig;

/// The backend that should handle a given request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Route to the existing Kiro (CodeWhisperer) backend.
    Kiro,
    /// Route to the Antigravity (Cloud Code) backend.
    Antigravity,
}

/// Routes requests to the appropriate backend based on model name.
#[derive(Debug, Clone)]
pub struct BackendRouter {
    /// Whether the antigravity backend is enabled.
    antigravity_enabled: bool,
}

impl BackendRouter {
    /// Creates a new router from the antigravity configuration.
    pub fn new(config: &AntigravityConfig) -> Self {
        Self {
            antigravity_enabled: config.enabled,
        }
    }

    /// Resolves which backend should handle a request for the given model.
    ///
    /// Returns `Backend::Antigravity` if:
    /// - The antigravity backend is enabled, AND
    /// - The model is in the antigravity model set
    ///
    /// Otherwise returns `Backend::Kiro`.
    pub fn resolve(&self, model_name: &str) -> Backend {
        if self.antigravity_enabled && is_antigravity_model(model_name) {
            Backend::Antigravity
        } else {
            Backend::Kiro
        }
    }

    /// Returns whether the antigravity backend is enabled.
    pub fn is_antigravity_enabled(&self) -> bool {
        self.antigravity_enabled
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(enabled: bool) -> AntigravityConfig {
        AntigravityConfig {
            enabled,
            refresh_token: None,
            project_id: None,
        }
    }

    #[test]
    fn test_resolve_antigravity_enabled() {
        let router = BackendRouter::new(&make_config(true));

        // Antigravity models should route to Antigravity
        assert_eq!(
            router.resolve("claude-opus-4-6-thinking"),
            Backend::Antigravity
        );
        assert_eq!(
            router.resolve("claude-sonnet-4-5-thinking"),
            Backend::Antigravity
        );
        assert_eq!(router.resolve("claude-sonnet-4-5"), Backend::Antigravity);
        assert_eq!(router.resolve("gemini-3-flash"), Backend::Antigravity);
        assert_eq!(router.resolve("gemini-3-pro-low"), Backend::Antigravity);
        assert_eq!(router.resolve("gemini-3-pro-high"), Backend::Antigravity);
    }

    #[test]
    fn test_resolve_kiro_models_when_antigravity_enabled() {
        let router = BackendRouter::new(&make_config(true));

        // Non-antigravity models should still route to Kiro
        assert_eq!(router.resolve("claude-sonnet-4"), Backend::Kiro);
        assert_eq!(router.resolve("claude-haiku-4.5"), Backend::Kiro);
        assert_eq!(router.resolve("auto"), Backend::Kiro);
        assert_eq!(router.resolve("gpt-4"), Backend::Kiro);
    }

    #[test]
    fn test_resolve_antigravity_disabled() {
        let router = BackendRouter::new(&make_config(false));

        // Everything routes to Kiro when antigravity is disabled
        assert_eq!(router.resolve("claude-opus-4-6-thinking"), Backend::Kiro);
        assert_eq!(router.resolve("gemini-3-flash"), Backend::Kiro);
        assert_eq!(router.resolve("claude-sonnet-4"), Backend::Kiro);
    }

    #[test]
    fn test_is_antigravity_enabled() {
        let enabled = BackendRouter::new(&make_config(true));
        assert!(enabled.is_antigravity_enabled());

        let disabled = BackendRouter::new(&make_config(false));
        assert!(!disabled.is_antigravity_enabled());
    }

    #[test]
    fn test_resolve_empty_model() {
        let router = BackendRouter::new(&make_config(true));
        assert_eq!(router.resolve(""), Backend::Kiro);
    }
}
