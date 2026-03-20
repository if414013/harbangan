use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use dashmap::DashMap;
use tracing::debug;
use uuid::Uuid;

use crate::providers::types::ProviderId;

/// Identifies a specific provider account for rate-limit tracking.
///
/// Combines the user (or admin pool), provider, and account label
/// to uniquely key rate-limit state.
#[derive(Debug, Clone)]
pub struct AccountId {
    /// None = admin pool account
    pub user_id: Option<Uuid>,
    pub provider_id: ProviderId,
    pub account_label: String,
}

impl PartialEq for AccountId {
    fn eq(&self, other: &Self) -> bool {
        self.user_id == other.user_id
            && self.provider_id == other.provider_id
            && self.account_label == other.account_label
    }
}

impl Eq for AccountId {}

impl Hash for AccountId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.user_id.hash(state);
        self.provider_id.hash(state);
        self.account_label.hash(state);
    }
}

/// Tracked rate-limit state for a single provider account.
pub struct RateLimitState {
    pub requests_remaining: Option<u64>,
    pub tokens_remaining: Option<u64>,
    #[allow(dead_code)]
    pub reset_at: Option<Instant>,
    pub limited_until: Option<Instant>,
    pub updated_at: Instant,
}

/// Tracks per-account rate-limit headroom and selects the best account
/// for each request using a score-based algorithm.
///
/// **Limitation: process-local state.** All rate-limit data lives in an
/// in-memory `DashMap`. When running multiple replicas behind a load balancer,
/// each replica maintains its own independent view of rate-limit headroom.
/// This means replicas may route to an account that another replica has already
/// exhausted. For single-replica deployments (the current default), this is fine.
///
/// To support multi-replica deployments, implement the [`RateLimitStore`] trait
/// with a shared backend (e.g. Redis or PostgreSQL) and replace the `DashMap`
/// with that implementation.
pub struct RateLimitTracker {
    states: DashMap<AccountId, RateLimitState>,
    round_robin: AtomicU64,
}

/// Default score when no rate-limit data has been recorded for an account.
/// High value assumes the account is available.
const DEFAULT_SCORE: u64 = u64::MAX / 2;

/// Default retry-after duration when a 429 has no Retry-After header.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(60);

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            states: DashMap::new(),
            round_robin: AtomicU64::new(0),
        }
    }

    /// Parse provider-specific rate-limit headers and update tracked state.
    pub fn update_from_headers(
        &self,
        account_id: &AccountId,
        provider_id: &ProviderId,
        headers: &HeaderMap,
    ) {
        let (requests_remaining, tokens_remaining) = match provider_id {
            ProviderId::Anthropic => (
                parse_header(headers, "anthropic-ratelimit-requests-remaining"),
                parse_header(headers, "anthropic-ratelimit-tokens-remaining"),
            ),
            ProviderId::OpenAICodex => (
                parse_header(headers, "x-ratelimit-remaining-requests"),
                parse_header(headers, "x-ratelimit-remaining-tokens"),
            ),
            ProviderId::Copilot => (parse_header(headers, "x-ratelimit-remaining"), None),
            _ => {
                // Best-effort: try OpenAI-style headers, skip if not found
                let req = parse_header(headers, "x-ratelimit-remaining-requests");
                let tok = parse_header(headers, "x-ratelimit-remaining-tokens");
                if req.is_none() && tok.is_none() {
                    return;
                }
                (req, tok)
            }
        };

        // Only update if we got at least one value
        if requests_remaining.is_none() && tokens_remaining.is_none() {
            return;
        }

        debug!(
            provider = %provider_id,
            label = %account_id.account_label,
            requests_remaining = ?requests_remaining,
            tokens_remaining = ?tokens_remaining,
            "Updated rate-limit state from headers"
        );

        self.states.insert(
            account_id.clone(),
            RateLimitState {
                requests_remaining,
                tokens_remaining,
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );
    }

    /// Mark an account as rate-limited (e.g., after receiving a 429).
    pub fn mark_limited(&self, account_id: &AccountId, retry_after: Option<Duration>) {
        let cooldown = retry_after.unwrap_or(DEFAULT_RETRY_AFTER);
        let now = Instant::now();

        debug!(
            provider = %account_id.provider_id,
            label = %account_id.account_label,
            cooldown_secs = cooldown.as_secs(),
            "Marking account as rate-limited"
        );

        self.states
            .entry(account_id.clone())
            .and_modify(|state| {
                state.limited_until = Some(now + cooldown);
                state.requests_remaining = Some(0);
                state.updated_at = now;
            })
            .or_insert(RateLimitState {
                requests_remaining: Some(0),
                tokens_remaining: None,
                reset_at: None,
                limited_until: Some(now + cooldown),
                updated_at: now,
            });
    }

    /// Select the best account from candidates based on rate-limit headroom.
    ///
    /// Returns None if all candidates are currently rate-limited.
    pub fn best_account(&self, candidates: &[AccountId]) -> Option<AccountId> {
        if candidates.is_empty() {
            return None;
        }

        let now = Instant::now();

        // Score each candidate, filtering out rate-limited ones
        let mut scored: Vec<(usize, u64)> = Vec::new();
        for (idx, candidate) in candidates.iter().enumerate() {
            match self.states.get(candidate) {
                Some(state) => {
                    // Skip if actively rate-limited
                    if let Some(limited_until) = state.limited_until {
                        if limited_until > now {
                            continue;
                        }
                    }
                    let score = compute_score(state.requests_remaining, state.tokens_remaining);
                    scored.push((idx, score));
                }
                None => {
                    // No data recorded — assume available with high default score
                    scored.push((idx, DEFAULT_SCORE));
                }
            }
        }

        if scored.is_empty() {
            return None;
        }

        // Find the maximum score
        let max_score = scored.iter().map(|(_, s)| *s).max().unwrap();

        // Collect all candidates tied at max score
        let tied: Vec<usize> = scored
            .iter()
            .filter(|(_, s)| *s == max_score)
            .map(|(idx, _)| *idx)
            .collect();

        // Tiebreak with round-robin
        let rr = self.round_robin.fetch_add(1, Ordering::Relaxed);
        let winner_idx = tied[rr as usize % tied.len()];

        Some(candidates[winner_idx].clone())
    }

    /// Select the best account from candidates grouped by priority tier.
    ///
    /// Candidates are `(AccountId, priority)` where lower priority number = better.
    /// Within the best available tier, selects by headroom score + round-robin tiebreak.
    /// Falls back to next tier if the best tier is fully rate-limited.
    /// Returns None if all candidates are currently rate-limited.
    pub fn best_account_with_priority(&self, candidates: &[(AccountId, i32)]) -> Option<AccountId> {
        if candidates.is_empty() {
            return None;
        }

        // Group candidates by priority tier
        let mut tiers: std::collections::BTreeMap<i32, Vec<&AccountId>> =
            std::collections::BTreeMap::new();
        for (aid, pri) in candidates {
            tiers.entry(*pri).or_default().push(aid);
        }

        // Try each tier in priority order (lowest number first)
        for tier_accounts in tiers.values() {
            let tier_ids: Vec<AccountId> = tier_accounts.iter().map(|a| (*a).clone()).collect();
            if let Some(best) = self.best_account(&tier_ids) {
                return Some(best);
            }
            // All in this tier are rate-limited — try next tier
        }

        None
    }

    /// Remove entries that haven't been updated within `max_age`.
    #[allow(dead_code)]
    pub fn cleanup_stale(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.states.retain(|_, state| state.updated_at > cutoff);
    }

    /// Get a snapshot of all tracked rate-limit states.
    pub fn get_all_states(&self) -> Vec<(AccountId, RateLimitStateSnapshot)> {
        self.states
            .iter()
            .map(|entry| {
                let id = entry.key().clone();
                let state = entry.value();
                (
                    id,
                    RateLimitStateSnapshot {
                        requests_remaining: state.requests_remaining,
                        tokens_remaining: state.tokens_remaining,
                        limited_until: state.limited_until,
                    },
                )
            })
            .collect()
    }
}

/// Snapshot of rate-limit state for API responses.
pub struct RateLimitStateSnapshot {
    pub requests_remaining: Option<u64>,
    pub tokens_remaining: Option<u64>,
    pub limited_until: Option<Instant>,
}

/// Trait abstracting rate-limit state storage.
///
/// The default [`RateLimitTracker`] is process-local. Implement this trait
/// with a shared backend (Redis, PostgreSQL) to support multi-replica
/// deployments where rate-limit headroom must be visible across all replicas.
#[allow(dead_code)]
pub trait RateLimitStore: Send + Sync {
    /// Record a successful response's rate-limit headers.
    fn update_from_headers(
        &self,
        account_id: &AccountId,
        provider_id: &ProviderId,
        headers: &HeaderMap,
    );

    /// Mark an account as rate-limited after a 429 response.
    fn mark_limited(&self, account_id: &AccountId, retry_after: Option<Duration>);

    /// Select the best account from candidates based on headroom.
    fn best_account(&self, candidates: &[AccountId]) -> Option<AccountId>;

    /// Select the best account considering priority tiers.
    fn best_account_with_priority(&self, candidates: &[(AccountId, i32)]) -> Option<AccountId>;
}

/// Compute a score for an account based on remaining headroom.
/// Higher score = more headroom = preferred.
fn compute_score(requests_remaining: Option<u64>, tokens_remaining: Option<u64>) -> u64 {
    let req_score = requests_remaining.unwrap_or(0).saturating_mul(100);
    let tok_score = tokens_remaining.unwrap_or(0) / 1000;
    req_score.saturating_add(tok_score)
}

/// Parse a single header value as u64.
fn parse_header(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(provider: ProviderId, label: &str) -> AccountId {
        AccountId {
            user_id: None,
            provider_id: provider,
            account_label: label.to_string(),
        }
    }

    fn make_user_account(user_id: Uuid, provider: ProviderId, label: &str) -> AccountId {
        AccountId {
            user_id: Some(user_id),
            provider_id: provider,
            account_label: label.to_string(),
        }
    }

    #[test]
    fn test_best_account_picks_highest_headroom() {
        let tracker = RateLimitTracker::new();

        let acct_a = make_account(ProviderId::Anthropic, "team-a");
        let acct_b = make_account(ProviderId::Anthropic, "team-b");

        // acct_a: 100 requests remaining
        tracker.states.insert(
            acct_a.clone(),
            RateLimitState {
                requests_remaining: Some(100),
                tokens_remaining: Some(50_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        // acct_b: 500 requests remaining — should win
        tracker.states.insert(
            acct_b.clone(),
            RateLimitState {
                requests_remaining: Some(500),
                tokens_remaining: Some(50_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        let best = tracker.best_account(&[acct_a, acct_b.clone()]).unwrap();
        assert_eq!(best.account_label, "team-b");
    }

    #[test]
    fn test_mark_limited_excludes_account() {
        let tracker = RateLimitTracker::new();

        let acct_a = make_account(ProviderId::OpenAICodex, "key-1");
        let acct_b = make_account(ProviderId::OpenAICodex, "key-2");

        // Both have headroom
        for acct in [&acct_a, &acct_b] {
            tracker.states.insert(
                acct.clone(),
                RateLimitState {
                    requests_remaining: Some(200),
                    tokens_remaining: Some(100_000),
                    reset_at: None,
                    limited_until: None,
                    updated_at: Instant::now(),
                },
            );
        }

        // Mark acct_a as limited
        tracker.mark_limited(&acct_a, Some(Duration::from_secs(120)));

        let best = tracker
            .best_account(&[acct_a.clone(), acct_b.clone()])
            .unwrap();
        assert_eq!(best.account_label, "key-2");
    }

    #[test]
    fn test_cleanup_removes_stale_entries() {
        let tracker = RateLimitTracker::new();

        let acct = make_account(ProviderId::Anthropic, "stale");

        // Insert with updated_at far in the past
        tracker.states.insert(
            acct.clone(),
            RateLimitState {
                requests_remaining: Some(100),
                tokens_remaining: None,
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now() - Duration::from_secs(700),
            },
        );

        assert_eq!(tracker.states.len(), 1);

        // Cleanup with 10-min max age — should remove the stale entry
        tracker.cleanup_stale(Duration::from_secs(600));

        assert_eq!(tracker.states.len(), 0);
    }

    #[test]
    fn test_update_from_headers_anthropic() {
        let tracker = RateLimitTracker::new();
        let acct = make_account(ProviderId::Anthropic, "ant-1");

        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-requests-remaining",
            "42".parse().unwrap(),
        );
        headers.insert(
            "anthropic-ratelimit-tokens-remaining",
            "80000".parse().unwrap(),
        );

        tracker.update_from_headers(&acct, &ProviderId::Anthropic, &headers);

        let state = tracker.states.get(&acct).unwrap();
        assert_eq!(state.requests_remaining, Some(42));
        assert_eq!(state.tokens_remaining, Some(80_000));
    }

    #[test]
    fn test_update_from_headers_openai() {
        let tracker = RateLimitTracker::new();
        let acct = make_account(ProviderId::OpenAICodex, "oai-1");

        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining-requests", "150".parse().unwrap());
        headers.insert("x-ratelimit-remaining-tokens", "200000".parse().unwrap());

        tracker.update_from_headers(&acct, &ProviderId::OpenAICodex, &headers);

        let state = tracker.states.get(&acct).unwrap();
        assert_eq!(state.requests_remaining, Some(150));
        assert_eq!(state.tokens_remaining, Some(200_000));
    }

    #[test]
    fn test_round_robin_tiebreak() {
        let tracker = RateLimitTracker::new();

        let acct_a = make_account(ProviderId::Anthropic, "a");
        let acct_b = make_account(ProviderId::Anthropic, "b");

        // Both have identical headroom
        for acct in [&acct_a, &acct_b] {
            tracker.states.insert(
                acct.clone(),
                RateLimitState {
                    requests_remaining: Some(100),
                    tokens_remaining: Some(50_000),
                    reset_at: None,
                    limited_until: None,
                    updated_at: Instant::now(),
                },
            );
        }

        let candidates = vec![acct_a.clone(), acct_b.clone()];

        // Call multiple times — should alternate between a and b
        let mut labels = Vec::new();
        for _ in 0..4 {
            let best = tracker.best_account(&candidates).unwrap();
            labels.push(best.account_label.clone());
        }

        // Should contain both labels (round-robin alternates)
        assert!(labels.contains(&"a".to_string()));
        assert!(labels.contains(&"b".to_string()));
    }

    #[test]
    fn test_no_data_returns_high_default_score() {
        let tracker = RateLimitTracker::new();

        // acct_a: no data (never seen)
        let acct_a = make_account(ProviderId::Anthropic, "unknown");
        // acct_b: low headroom
        let acct_b = make_account(ProviderId::Anthropic, "low");

        tracker.states.insert(
            acct_b.clone(),
            RateLimitState {
                requests_remaining: Some(1),
                tokens_remaining: Some(500),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        // acct_a should win because DEFAULT_SCORE >> low headroom score
        let best = tracker.best_account(&[acct_a.clone(), acct_b]).unwrap();
        assert_eq!(best.account_label, "unknown");
    }

    #[test]
    fn test_all_limited_returns_none() {
        let tracker = RateLimitTracker::new();

        let acct_a = make_account(ProviderId::OpenAICodex, "x");
        let acct_b = make_account(ProviderId::OpenAICodex, "y");

        // Both rate-limited
        for acct in [&acct_a, &acct_b] {
            tracker.mark_limited(acct, Some(Duration::from_secs(300)));
        }

        let result = tracker.best_account(&[acct_a, acct_b]);
        assert!(result.is_none());
    }

    #[test]
    fn test_account_id_equality() {
        let a = make_account(ProviderId::Anthropic, "team-1");
        let b = make_account(ProviderId::Anthropic, "team-1");
        let c = make_account(ProviderId::Anthropic, "team-2");

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_account_id_hash_consistency() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        let acct = make_account(ProviderId::Copilot, "cop-1");
        map.insert(acct.clone(), 42);

        let lookup = make_account(ProviderId::Copilot, "cop-1");
        assert_eq!(map.get(&lookup), Some(&42));
    }

    #[test]
    fn test_user_account_distinct_from_admin() {
        let user_id = Uuid::new_v4();
        let admin_acct = make_account(ProviderId::Anthropic, "shared");
        let user_acct = make_user_account(user_id, ProviderId::Anthropic, "shared");

        assert_ne!(admin_acct, user_acct);
    }

    #[test]
    fn test_empty_candidates_returns_none() {
        let tracker = RateLimitTracker::new();
        assert!(tracker.best_account(&[]).is_none());
    }

    #[test]
    fn test_expired_limit_allows_selection() {
        let tracker = RateLimitTracker::new();
        let acct = make_account(ProviderId::Anthropic, "expired");

        // Insert with limited_until in the past
        tracker.states.insert(
            acct.clone(),
            RateLimitState {
                requests_remaining: Some(50),
                tokens_remaining: Some(10_000),
                reset_at: None,
                limited_until: Some(Instant::now() - Duration::from_secs(10)),
                updated_at: Instant::now(),
            },
        );

        let best = tracker.best_account(std::slice::from_ref(&acct)).unwrap();
        assert_eq!(best.account_label, "expired");
    }

    #[test]
    fn test_update_from_headers_no_headers_skips() {
        let tracker = RateLimitTracker::new();
        let acct = make_account(ProviderId::Anthropic, "empty");

        let headers = HeaderMap::new();
        tracker.update_from_headers(&acct, &ProviderId::Anthropic, &headers);

        // No state should be inserted
        assert!(tracker.states.get(&acct).is_none());
    }

    #[test]
    fn test_update_from_headers_copilot() {
        let tracker = RateLimitTracker::new();
        let acct = make_account(ProviderId::Copilot, "cop-1");

        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "25".parse().unwrap());

        tracker.update_from_headers(&acct, &ProviderId::Copilot, &headers);

        let state = tracker.states.get(&acct).unwrap();
        assert_eq!(state.requests_remaining, Some(25));
        assert_eq!(state.tokens_remaining, None);
    }

    #[test]
    fn test_mark_limited_default_retry_after() {
        let tracker = RateLimitTracker::new();
        let acct = make_account(ProviderId::Anthropic, "default-retry");

        let before = Instant::now();
        tracker.mark_limited(&acct, None);
        let after = Instant::now();

        let state = tracker.states.get(&acct).unwrap();
        let limited_until = state.limited_until.unwrap();

        // Should be ~60 seconds from now
        assert!(limited_until >= before + Duration::from_secs(59));
        assert!(limited_until <= after + Duration::from_secs(61));
    }

    #[test]
    fn test_cleanup_preserves_fresh_entries() {
        let tracker = RateLimitTracker::new();

        let stale = make_account(ProviderId::Anthropic, "stale");
        let fresh = make_account(ProviderId::Anthropic, "fresh");

        tracker.states.insert(
            stale.clone(),
            RateLimitState {
                requests_remaining: Some(10),
                tokens_remaining: None,
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now() - Duration::from_secs(700),
            },
        );

        tracker.states.insert(
            fresh.clone(),
            RateLimitState {
                requests_remaining: Some(200),
                tokens_remaining: None,
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        tracker.cleanup_stale(Duration::from_secs(600));

        assert!(tracker.states.get(&stale).is_none());
        assert!(tracker.states.get(&fresh).is_some());
    }

    #[test]
    fn test_priority_prefers_higher_priority_tier() {
        let tracker = RateLimitTracker::new();

        let acct_p0 = make_account(ProviderId::Anthropic, "priority-0");
        let acct_p1 = make_account(ProviderId::Anthropic, "priority-1");

        // p0 has less headroom but higher priority
        tracker.states.insert(
            acct_p0.clone(),
            RateLimitState {
                requests_remaining: Some(10),
                tokens_remaining: Some(1_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );
        // p1 has more headroom but lower priority
        tracker.states.insert(
            acct_p1.clone(),
            RateLimitState {
                requests_remaining: Some(500),
                tokens_remaining: Some(100_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        let candidates = vec![(acct_p0, 0), (acct_p1, 1)];
        let best = tracker.best_account_with_priority(&candidates).unwrap();
        assert_eq!(best.account_label, "priority-0");
    }

    #[test]
    fn test_priority_falls_back_when_best_tier_limited() {
        let tracker = RateLimitTracker::new();

        let acct_p0 = make_account(ProviderId::Anthropic, "p0-limited");
        let acct_p1 = make_account(ProviderId::Anthropic, "p1-available");

        // p0 is rate-limited
        tracker.mark_limited(&acct_p0, Some(Duration::from_secs(300)));

        // p1 is available
        tracker.states.insert(
            acct_p1.clone(),
            RateLimitState {
                requests_remaining: Some(100),
                tokens_remaining: Some(50_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        let candidates = vec![(acct_p0, 0), (acct_p1, 1)];
        let best = tracker.best_account_with_priority(&candidates).unwrap();
        assert_eq!(best.account_label, "p1-available");
    }

    #[test]
    fn test_priority_same_tier_uses_headroom() {
        let tracker = RateLimitTracker::new();

        let acct_a = make_account(ProviderId::Anthropic, "same-tier-a");
        let acct_b = make_account(ProviderId::Anthropic, "same-tier-b");

        tracker.states.insert(
            acct_a.clone(),
            RateLimitState {
                requests_remaining: Some(50),
                tokens_remaining: Some(10_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );
        tracker.states.insert(
            acct_b.clone(),
            RateLimitState {
                requests_remaining: Some(200),
                tokens_remaining: Some(50_000),
                reset_at: None,
                limited_until: None,
                updated_at: Instant::now(),
            },
        );

        // Same priority — higher headroom wins
        let candidates = vec![(acct_a, 0), (acct_b, 0)];
        let best = tracker.best_account_with_priority(&candidates).unwrap();
        assert_eq!(best.account_label, "same-tier-b");
    }

    #[test]
    fn test_priority_all_limited_returns_none() {
        let tracker = RateLimitTracker::new();

        let acct_a = make_account(ProviderId::Anthropic, "all-lim-a");
        let acct_b = make_account(ProviderId::Anthropic, "all-lim-b");

        tracker.mark_limited(&acct_a, Some(Duration::from_secs(300)));
        tracker.mark_limited(&acct_b, Some(Duration::from_secs(300)));

        let candidates = vec![(acct_a, 0), (acct_b, 1)];
        assert!(tracker.best_account_with_priority(&candidates).is_none());
    }
}
