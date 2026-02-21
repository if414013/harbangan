//! Token bucket rate limiter for per-account request throttling.
//!
//! Each account gets a bucket that refills over time. Requests consume
//! tokens; when empty the account should be deprioritised.

use std::time::Instant;

// === Constants ===

const MAX_TOKENS: f64 = 50.0;
const REFILL_RATE: f64 = 6.0; // tokens per minute
const INITIAL_TOKENS: f64 = 50.0;

/// Token bucket for per-account rate limiting.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Current token count (may be fractional due to refill).
    tokens: f64,
    /// Last time tokens were refilled.
    last_refill: Instant,
}

impl TokenBucket {
    /// Creates a new bucket with full tokens.
    pub fn new() -> Self {
        Self {
            tokens: INITIAL_TOKENS,
            last_refill: Instant::now(),
        }
    }

    /// Returns the current token count after refilling.
    pub fn available(&self) -> f64 {
        let elapsed_minutes = self.last_refill.elapsed().as_secs_f64() / 60.0;
        let refilled = self.tokens + (elapsed_minutes * REFILL_RATE);
        refilled.min(MAX_TOKENS)
    }

    /// Returns the normalised fill ratio (0.0 to 1.0).
    pub fn fill_ratio(&self) -> f64 {
        self.available() / MAX_TOKENS
    }

    /// Attempts to consume one token. Returns true if successful.
    pub fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Returns true if at least one token is available.
    pub fn has_tokens(&self) -> bool {
        self.available() >= 1.0
    }

    /// Refunds one token (e.g., on failure before processing).
    pub fn refund(&mut self) {
        self.refill();
        self.tokens = (self.tokens + 1.0).min(MAX_TOKENS);
    }

    /// Materialise any accumulated refill.
    fn refill(&mut self) {
        let elapsed_minutes = self.last_refill.elapsed().as_secs_f64() / 60.0;
        self.tokens = (self.tokens + elapsed_minutes * REFILL_RATE).min(MAX_TOKENS);
        self.last_refill = Instant::now();
    }
}

impl Default for TokenBucket {
    fn default() -> Self {
        Self::new()
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_tokens() {
        let b = TokenBucket::new();
        assert!((b.available() - INITIAL_TOKENS).abs() < 1.0);
        assert!((b.fill_ratio() - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_consume_decreases_tokens() {
        let mut b = TokenBucket::new();
        let before = b.available();
        assert!(b.try_consume());
        assert!(b.available() < before);
    }

    #[test]
    fn test_consume_all_tokens() {
        let mut b = TokenBucket::new();
        for _ in 0..50 {
            assert!(b.try_consume());
        }
        // Should be empty now (or very close)
        assert!(!b.try_consume());
    }

    #[test]
    fn test_fill_ratio_range() {
        let b = TokenBucket::new();
        let ratio = b.fill_ratio();
        assert!((0.0..=1.0).contains(&ratio));
    }

    #[test]
    fn test_consume_returns_false_when_empty() {
        let mut b = TokenBucket::new();
        // Drain all tokens
        while b.try_consume() {}
        assert!(!b.try_consume());
    }

    #[test]
    fn test_has_tokens() {
        let mut b = TokenBucket::new();
        assert!(b.has_tokens());
        while b.try_consume() {}
        assert!(!b.has_tokens());
    }

    #[test]
    fn test_refund() {
        let mut b = TokenBucket::new();
        // Drain all tokens
        while b.try_consume() {}
        assert!(!b.has_tokens());

        // Refund one
        b.refund();
        assert!(b.has_tokens());
        assert!(b.try_consume());
    }
}
