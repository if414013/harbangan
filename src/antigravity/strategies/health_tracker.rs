//! Per-account health scoring for load balancing decisions.
//!
//! Tracks account health based on success/failure/rate-limit events
//! with time-based recovery.

use std::time::Instant;

// === Constants ===

const INITIAL_SCORE: f64 = 70.0;
const MIN_SCORE: f64 = 0.0;
const MAX_SCORE: f64 = 100.0;
const MIN_USABLE_SCORE: f64 = 50.0;

const SUCCESS_DELTA: f64 = 1.0;
const RATE_LIMIT_DELTA: f64 = -10.0;
const FAILURE_DELTA: f64 = -20.0;

/// Recovery rate: +10 points per hour
const RECOVERY_PER_HOUR: f64 = 10.0;

/// Per-account health tracker with time-based recovery.
#[derive(Debug, Clone)]
pub struct HealthTracker {
    /// Current raw score (before recovery adjustment).
    raw_score: f64,
    /// When the score was last updated.
    last_update: Instant,
}

impl HealthTracker {
    /// Creates a new tracker with the default initial score.
    pub fn new() -> Self {
        Self {
            raw_score: INITIAL_SCORE,
            last_update: Instant::now(),
        }
    }

    /// Returns the current health score, including time-based recovery.
    pub fn score(&self) -> f64 {
        let elapsed_hours = self.last_update.elapsed().as_secs_f64() / 3600.0;
        let recovered = self.raw_score + (elapsed_hours * RECOVERY_PER_HOUR);
        recovered.clamp(MIN_SCORE, MAX_SCORE)
    }

    /// Returns true if the account is healthy enough to use.
    pub fn is_usable(&self) -> bool {
        self.score() >= MIN_USABLE_SCORE
    }

    /// Record a successful request.
    pub fn notify_success(&mut self) {
        self.apply_and_snapshot(SUCCESS_DELTA);
    }

    /// Record a rate-limit response.
    pub fn notify_rate_limit(&mut self) {
        self.apply_and_snapshot(RATE_LIMIT_DELTA);
    }

    /// Record a failure (non-rate-limit error).
    pub fn notify_failure(&mut self) {
        self.apply_and_snapshot(FAILURE_DELTA);
    }

    /// Materialise recovery, apply delta, and reset the clock.
    fn apply_and_snapshot(&mut self, delta: f64) {
        let current = self.score();
        self.raw_score = (current + delta).clamp(MIN_SCORE, MAX_SCORE);
        self.last_update = Instant::now();
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_score() {
        let h = HealthTracker::new();
        assert!((h.score() - INITIAL_SCORE).abs() < 1.0);
        assert!(h.is_usable());
    }

    #[test]
    fn test_success_increases_score() {
        let mut h = HealthTracker::new();
        let before = h.score();
        h.notify_success();
        assert!(h.score() > before);
    }

    #[test]
    fn test_rate_limit_decreases_score() {
        let mut h = HealthTracker::new();
        let before = h.score();
        h.notify_rate_limit();
        assert!(h.score() < before);
    }

    #[test]
    fn test_failure_decreases_score() {
        let mut h = HealthTracker::new();
        let before = h.score();
        h.notify_failure();
        assert!(h.score() < before);
    }

    #[test]
    fn test_score_clamped_to_max() {
        let mut h = HealthTracker::new();
        // Push score up many times
        for _ in 0..200 {
            h.notify_success();
        }
        assert!(h.score() <= MAX_SCORE);
    }

    #[test]
    fn test_score_clamped_to_min() {
        let mut h = HealthTracker::new();
        // Push score down many times
        for _ in 0..20 {
            h.notify_failure();
        }
        assert!(h.score() >= MIN_SCORE);
    }

    #[test]
    fn test_usable_threshold() {
        let mut h = HealthTracker::new();
        // Initial 70, two failures = 70 - 20 - 20 = 30 < 50
        h.notify_failure();
        h.notify_failure();
        assert!(!h.is_usable());
    }

    #[test]
    fn test_failure_delta_values() {
        let mut h = HealthTracker::new();
        let before = h.score();
        h.notify_failure();
        let after = h.score();
        assert!((before - after - 20.0).abs() < 1.0);
    }

    #[test]
    fn test_rate_limit_delta_values() {
        let mut h = HealthTracker::new();
        let before = h.score();
        h.notify_rate_limit();
        let after = h.score();
        assert!((before - after - 10.0).abs() < 1.0);
    }
}
