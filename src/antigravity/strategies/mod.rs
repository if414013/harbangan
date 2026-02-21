//! Account selection strategy trait and registry.
//!
//! Defines the interface for pluggable account selection strategies
//! and re-exports the concrete implementations.

pub mod health_tracker;
pub mod hybrid;
pub mod quota_tracker;
pub mod round_robin;
pub mod sticky;
pub mod token_bucket;

use super::account_manager::AccountState;

/// Result of account selection: the chosen account index and optional wait time.
#[derive(Debug, Clone)]
pub struct SelectionResult {
    /// Index into the available accounts slice.
    pub index: usize,
    /// Milliseconds to wait before sending the request (0 = immediate).
    pub wait_ms: u64,
}

/// Trait for account selection strategies.
pub trait SelectionStrategy: Send + Sync + std::fmt::Debug {
    /// Select an account from the available accounts.
    ///
    /// Returns `None` if no account is available.
    fn select(&self, accounts: &[AccountState]) -> Option<SelectionResult>;

    /// Returns the strategy name for logging.
    fn name(&self) -> &'static str;
}

/// Strategy identifier for configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyKind {
    Sticky,
    RoundRobin,
    Hybrid,
}

impl StrategyKind {
    /// Parse from a string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sticky" => Self::Sticky,
            "round_robin" | "round-robin" | "roundrobin" => Self::RoundRobin,
            _ => Self::Hybrid, // default
        }
    }
}

/// Creates a boxed strategy from a kind identifier.
pub fn create_strategy(kind: StrategyKind) -> Box<dyn SelectionStrategy> {
    match kind {
        StrategyKind::Sticky => Box::new(sticky::StickyStrategy::new()),
        StrategyKind::RoundRobin => Box::new(round_robin::RoundRobinStrategy::new()),
        StrategyKind::Hybrid => Box::new(hybrid::HybridStrategy::new()),
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_kind_from_str() {
        assert_eq!(StrategyKind::from_str_loose("sticky"), StrategyKind::Sticky);
        assert_eq!(
            StrategyKind::from_str_loose("round_robin"),
            StrategyKind::RoundRobin
        );
        assert_eq!(
            StrategyKind::from_str_loose("round-robin"),
            StrategyKind::RoundRobin
        );
        assert_eq!(
            StrategyKind::from_str_loose("roundrobin"),
            StrategyKind::RoundRobin
        );
        assert_eq!(StrategyKind::from_str_loose("hybrid"), StrategyKind::Hybrid);
        assert_eq!(
            StrategyKind::from_str_loose("unknown"),
            StrategyKind::Hybrid
        );
        assert_eq!(StrategyKind::from_str_loose(""), StrategyKind::Hybrid);
    }

    #[test]
    fn test_create_strategy() {
        let s = create_strategy(StrategyKind::Sticky);
        assert_eq!(s.name(), "sticky");

        let s = create_strategy(StrategyKind::RoundRobin);
        assert_eq!(s.name(), "round_robin");

        let s = create_strategy(StrategyKind::Hybrid);
        assert_eq!(s.name(), "hybrid");
    }
}
