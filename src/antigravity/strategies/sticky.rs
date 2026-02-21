//! Sticky account selection strategy.
//!
//! Sticks to one account until it becomes unavailable (rate-limited),
//! then switches to the least-recently-used account. Best for prompt
//! caching continuity.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::{SelectionResult, SelectionStrategy};
use crate::antigravity::account_manager::AccountState;

/// Sticky strategy: prefer the current account, switch on failure.
#[derive(Debug)]
pub struct StickyStrategy {
    /// Index of the currently sticky account.
    current: AtomicUsize,
}

impl Default for StickyStrategy {
    fn default() -> Self {
        Self {
            current: AtomicUsize::new(0),
        }
    }
}

impl StickyStrategy {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SelectionStrategy for StickyStrategy {
    fn select(&self, accounts: &[AccountState]) -> Option<SelectionResult> {
        if accounts.is_empty() {
            return None;
        }

        let current = self.current.load(Ordering::Relaxed);

        // Try the current sticky account first
        if current < accounts.len() {
            return Some(SelectionResult {
                index: current,
                wait_ms: 0,
            });
        }

        // Current index is out of range (account removed), pick LRU
        let lru_index = pick_lru(accounts);
        self.current.store(lru_index, Ordering::Relaxed);

        Some(SelectionResult {
            index: lru_index,
            wait_ms: 0,
        })
    }

    fn name(&self) -> &'static str {
        "sticky"
    }
}

/// Picks the least-recently-used account (or first if none have been used).
fn pick_lru(accounts: &[AccountState]) -> usize {
    accounts
        .iter()
        .enumerate()
        .min_by_key(|(_, a)| a.last_used)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(email: &str) -> AccountState {
        AccountState::new(email.to_string(), "token".to_string())
    }

    #[test]
    fn test_sticky_selects_first() {
        let strategy = StickyStrategy::new();
        let accounts = vec![make_account("a@test.com"), make_account("b@test.com")];

        let result = strategy.select(&accounts).unwrap();
        assert_eq!(result.index, 0);
        assert_eq!(result.wait_ms, 0);
    }

    #[test]
    fn test_sticky_stays_on_same() {
        let strategy = StickyStrategy::new();
        let accounts = vec![make_account("a@test.com"), make_account("b@test.com")];

        let r1 = strategy.select(&accounts).unwrap();
        let r2 = strategy.select(&accounts).unwrap();
        assert_eq!(r1.index, r2.index);
    }

    #[test]
    fn test_sticky_empty_accounts() {
        let strategy = StickyStrategy::new();
        assert!(strategy.select(&[]).is_none());
    }

    #[test]
    fn test_sticky_falls_back_to_lru() {
        let strategy = StickyStrategy::new();
        // Set current to out-of-range
        strategy.current.store(99, Ordering::Relaxed);

        let accounts = vec![make_account("a@test.com"), make_account("b@test.com")];
        let result = strategy.select(&accounts).unwrap();
        // Should pick LRU (both unused, so index 0)
        assert_eq!(result.index, 0);
    }

    #[test]
    fn test_sticky_name() {
        let strategy = StickyStrategy::new();
        assert_eq!(strategy.name(), "sticky");
    }
}
