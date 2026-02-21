//! Hybrid account selection strategy.
//!
//! Weighted scoring that balances health, token availability, quota,
//! and recency. Default strategy for production use.
//!
//! Weights: health(2) + tokens(5) + quota(3) + lru(0.1)

use std::time::Instant;

use super::{SelectionResult, SelectionStrategy};
use crate::antigravity::account_manager::AccountState;

// === Scoring Weights ===

const WEIGHT_HEALTH: f64 = 2.0;
const WEIGHT_TOKENS: f64 = 5.0;
const WEIGHT_QUOTA: f64 = 3.0;
const WEIGHT_LRU: f64 = 0.1;

/// Hybrid strategy: weighted multi-factor scoring.
#[derive(Debug)]
pub struct HybridStrategy;

impl Default for HybridStrategy {
    fn default() -> Self {
        Self
    }
}

impl HybridStrategy {
    pub fn new() -> Self {
        Self
    }
}

impl SelectionStrategy for HybridStrategy {
    fn select(&self, accounts: &[AccountState]) -> Option<SelectionResult> {
        if accounts.is_empty() {
            return None;
        }

        let now = Instant::now();

        let best = accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let score = compute_score(a, now);
                (i, score)
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        best.map(|(index, _)| SelectionResult { index, wait_ms: 0 })
    }

    fn name(&self) -> &'static str {
        "hybrid"
    }
}

/// Computes the weighted score for an account.
///
/// Reference formula: score = (Health * 2) + ((Tokens/Max * 100) * 5) + (Quota_score * 3) + (LRU_seconds * 0.1)
fn compute_score(account: &AccountState, now: Instant) -> f64 {
    // Health: raw score 0-100
    let health = account.health.score();

    // Token bucket: (fill_ratio * 100) to get 0-100 range
    let token_score = account.token_bucket.fill_ratio() * 100.0;

    // Quota: fill_ratio * 100 to get 0-100 range (100 if no data)
    let quota_score = account.quota.fill_ratio() * 100.0;

    // LRU: raw seconds since last use
    let lru_seconds = account
        .last_used
        .map(|t| now.checked_duration_since(t).unwrap_or_default().as_secs_f64())
        .unwrap_or(3600.0); // never used = treat as 1 hour idle

    (health * WEIGHT_HEALTH)
        + (token_score * WEIGHT_TOKENS)
        + (quota_score * WEIGHT_QUOTA)
        + (lru_seconds * WEIGHT_LRU)
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(email: &str) -> AccountState {
        AccountState::new(email.to_string(), "token".to_string())
    }

    #[test]
    fn test_hybrid_selects_best() {
        let strategy = HybridStrategy::new();
        let accounts = vec![make_account("a@test.com"), make_account("b@test.com")];

        // Both accounts are identical, should pick one
        let result = strategy.select(&accounts).unwrap();
        assert!(result.index < 2);
    }

    #[test]
    fn test_hybrid_prefers_healthier() {
        let strategy = HybridStrategy::new();
        let mut accounts = vec![make_account("a@test.com"), make_account("b@test.com")];

        // Damage account a's health
        accounts[0].health.notify_failure();
        accounts[0].health.notify_failure();

        let result = strategy.select(&accounts).unwrap();
        assert_eq!(result.index, 1); // b should be preferred
    }

    #[test]
    fn test_hybrid_empty() {
        let strategy = HybridStrategy::new();
        assert!(strategy.select(&[]).is_none());
    }

    #[test]
    fn test_hybrid_single_account() {
        let strategy = HybridStrategy::new();
        let accounts = vec![make_account("a@test.com")];

        let result = strategy.select(&accounts).unwrap();
        assert_eq!(result.index, 0);
    }

    #[test]
    fn test_hybrid_name() {
        let strategy = HybridStrategy::new();
        assert_eq!(strategy.name(), "hybrid");
    }

    #[test]
    fn test_compute_score_positive() {
        let account = make_account("a@test.com");
        let score = compute_score(&account, Instant::now());
        assert!(score > 0.0);
    }

    #[test]
    fn test_compute_score_decreases_with_failures() {
        let healthy = make_account("a@test.com");
        let mut unhealthy = make_account("b@test.com");

        unhealthy.health.notify_failure();
        unhealthy.health.notify_failure();

        let now = Instant::now();
        let score_healthy = compute_score(&healthy, now);
        let score_unhealthy = compute_score(&unhealthy, now);

        assert!(score_healthy > score_unhealthy);
    }
}
