//! Round-robin account selection strategy.
//!
//! Cycles through accounts evenly, skipping unavailable ones.
//! Best for even load distribution across accounts.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::{SelectionResult, SelectionStrategy};
use crate::antigravity::account_manager::AccountState;

/// Round-robin strategy: cycle through accounts in order.
#[derive(Debug)]
pub struct RoundRobinStrategy {
    /// Next index to try.
    next: AtomicUsize,
}

impl Default for RoundRobinStrategy {
    fn default() -> Self {
        Self {
            next: AtomicUsize::new(0),
        }
    }
}

impl RoundRobinStrategy {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SelectionStrategy for RoundRobinStrategy {
    fn select(&self, accounts: &[AccountState]) -> Option<SelectionResult> {
        if accounts.is_empty() {
            return None;
        }

        let start = self.next.fetch_add(1, Ordering::Relaxed) % accounts.len();

        // The accounts passed in are already filtered to available ones,
        // so just pick the next one in rotation.
        let index = start % accounts.len();

        Some(SelectionResult { index, wait_ms: 0 })
    }

    fn name(&self) -> &'static str {
        "round_robin"
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(email: &str) -> AccountState {
        AccountState::new(email.to_string(), "token".to_string())
    }

    #[test]
    fn test_round_robin_cycles() {
        let strategy = RoundRobinStrategy::new();
        let accounts = vec![
            make_account("a@test.com"),
            make_account("b@test.com"),
            make_account("c@test.com"),
        ];

        let r1 = strategy.select(&accounts).unwrap();
        let r2 = strategy.select(&accounts).unwrap();
        let r3 = strategy.select(&accounts).unwrap();
        let r4 = strategy.select(&accounts).unwrap();

        assert_eq!(r1.index, 0);
        assert_eq!(r2.index, 1);
        assert_eq!(r3.index, 2);
        assert_eq!(r4.index, 0); // wraps around
    }

    #[test]
    fn test_round_robin_single_account() {
        let strategy = RoundRobinStrategy::new();
        let accounts = vec![make_account("a@test.com")];

        let r1 = strategy.select(&accounts).unwrap();
        let r2 = strategy.select(&accounts).unwrap();
        assert_eq!(r1.index, 0);
        assert_eq!(r2.index, 0);
    }

    #[test]
    fn test_round_robin_empty() {
        let strategy = RoundRobinStrategy::new();
        assert!(strategy.select(&[]).is_none());
    }

    #[test]
    fn test_round_robin_name() {
        let strategy = RoundRobinStrategy::new();
        assert_eq!(strategy.name(), "round_robin");
    }
}
