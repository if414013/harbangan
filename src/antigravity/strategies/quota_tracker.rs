//! Quota awareness tracking from rate limit response headers.
//!
//! Tracks remaining quota per account so strategies can deprioritise
//! accounts approaching their limits.

use std::time::{Duration, Instant};

// === Constants ===

/// Quota data is considered stale after this duration.
const STALE_AFTER: Duration = Duration::from_secs(5 * 60);

/// Low quota threshold (10% remaining).
const LOW_THRESHOLD: f64 = 0.10;

/// Critical quota threshold (5% remaining).
const CRITICAL_THRESHOLD: f64 = 0.05;

/// Per-account quota tracker.
#[derive(Debug, Clone)]
pub struct QuotaTracker {
    /// Remaining quota (from response headers).
    remaining: Option<u64>,
    /// Total quota limit (from response headers).
    limit: Option<u64>,
    /// When the quota was last updated.
    last_update: Option<Instant>,
}

impl QuotaTracker {
    /// Creates a new tracker with no quota information.
    pub fn new() -> Self {
        Self {
            remaining: None,
            limit: None,
            last_update: None,
        }
    }

    /// Updates quota from rate limit response headers.
    pub fn update(&mut self, remaining: u64, limit: u64) {
        self.remaining = Some(remaining);
        self.limit = Some(limit);
        self.last_update = Some(Instant::now());
    }

    /// Returns the quota fill ratio (0.0 = empty, 1.0 = full).
    /// Returns 1.0 if no quota data is available (optimistic default).
    pub fn fill_ratio(&self) -> f64 {
        if self.is_stale() {
            return 1.0;
        }
        match (self.remaining, self.limit) {
            (Some(remaining), Some(limit)) if limit > 0 => remaining as f64 / limit as f64,
            _ => 1.0,
        }
    }

    /// Returns true if quota is below the low threshold.
    pub fn is_low(&self) -> bool {
        self.fill_ratio() < LOW_THRESHOLD
    }

    /// Returns true if quota is below the critical threshold.
    pub fn is_critical(&self) -> bool {
        self.fill_ratio() < CRITICAL_THRESHOLD
    }

    /// Returns true if quota data is stale or unavailable.
    pub fn is_stale(&self) -> bool {
        match self.last_update {
            Some(t) => t.elapsed() > STALE_AFTER,
            None => true,
        }
    }

    /// Returns true if we have any quota data (even if stale).
    pub fn has_data(&self) -> bool {
        self.remaining.is_some() && self.limit.is_some()
    }
}

impl Default for QuotaTracker {
    fn default() -> Self {
        Self::new()
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let q = QuotaTracker::new();
        assert!(!q.has_data());
        assert!(q.is_stale());
        assert!((q.fill_ratio() - 1.0).abs() < f64::EPSILON);
        assert!(!q.is_low());
        assert!(!q.is_critical());
    }

    #[test]
    fn test_update_and_fill_ratio() {
        let mut q = QuotaTracker::new();
        q.update(50, 100);
        assert!(q.has_data());
        assert!(!q.is_stale());
        assert!((q.fill_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_low_threshold() {
        let mut q = QuotaTracker::new();
        q.update(5, 100); // 5% remaining
        assert!(q.is_low());
    }

    #[test]
    fn test_critical_threshold() {
        let mut q = QuotaTracker::new();
        q.update(2, 100); // 2% remaining
        assert!(q.is_critical());
        assert!(q.is_low()); // critical implies low
    }

    #[test]
    fn test_not_low_when_healthy() {
        let mut q = QuotaTracker::new();
        q.update(80, 100);
        assert!(!q.is_low());
        assert!(!q.is_critical());
    }

    #[test]
    fn test_zero_limit() {
        let mut q = QuotaTracker::new();
        q.update(0, 0);
        // Should return optimistic default
        assert!((q.fill_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_full_quota() {
        let mut q = QuotaTracker::new();
        q.update(100, 100);
        assert!((q.fill_ratio() - 1.0).abs() < f64::EPSILON);
    }
}
