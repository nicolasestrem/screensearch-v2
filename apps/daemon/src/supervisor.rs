//! Bounded restart policy for the supervised model worker.
//!
//! The restart budget, sliding window, and backoff are code constants rather than
//! environment variables: the production spec names no model-worker restart tunables,
//! and operating rule 5 forbids adding undocumented ones.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Decision returned after a model-worker exit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestartDecision {
    /// Respawn the worker after waiting for `after`.
    Restart {
        /// Backoff to wait before respawning.
        after: Duration,
    },
    /// The worker exited too many times inside the window; stop and fail loudly.
    GiveUp,
}

/// Sliding-window bounded restart policy with exponential backoff.
pub struct RestartPolicy {
    max_restarts: usize,
    window: Duration,
    base_backoff: Duration,
    max_backoff: Duration,
    failures: VecDeque<Instant>,
}

impl RestartPolicy {
    /// Production policy: at most five restarts per minute, backoff capped at ten seconds.
    pub fn new() -> Self {
        Self::with_limits(
            5,
            Duration::from_secs(60),
            Duration::from_millis(500),
            Duration::from_secs(10),
        )
    }

    fn with_limits(
        max_restarts: usize,
        window: Duration,
        base_backoff: Duration,
        max_backoff: Duration,
    ) -> Self {
        Self {
            max_restarts,
            window,
            base_backoff,
            max_backoff,
            failures: VecDeque::new(),
        }
    }

    /// Records an unexpected worker exit at `now` and decides whether to restart.
    ///
    /// Exits older than the sliding window are forgotten, so an occasional crash never
    /// accumulates toward the budget; a tight crash-loop exhausts it and yields `GiveUp`.
    pub fn on_exit(&mut self, now: Instant) -> RestartDecision {
        while let Some(oldest) = self.failures.front() {
            if now.saturating_duration_since(*oldest) > self.window {
                self.failures.pop_front();
            } else {
                break;
            }
        }
        self.failures.push_back(now);
        if self.failures.len() > self.max_restarts {
            return RestartDecision::GiveUp;
        }
        let exponent = u32::try_from(self.failures.len() - 1).unwrap_or(u32::MAX);
        let backoff = self
            .base_backoff
            .saturating_mul(2_u32.saturating_pow(exponent))
            .min(self.max_backoff);
        RestartDecision::Restart { after: backoff }
    }
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{RestartDecision, RestartPolicy};

    #[test]
    fn backoff_increases_within_the_restart_budget() {
        let mut policy = RestartPolicy::with_limits(
            5,
            Duration::from_secs(60),
            Duration::from_millis(500),
            Duration::from_secs(10),
        );
        let start = Instant::now();

        assert_eq!(
            policy.on_exit(start),
            RestartDecision::Restart {
                after: Duration::from_millis(500)
            }
        );
        assert_eq!(
            policy.on_exit(start + Duration::from_millis(1)),
            RestartDecision::Restart {
                after: Duration::from_secs(1)
            }
        );
        assert_eq!(
            policy.on_exit(start + Duration::from_millis(2)),
            RestartDecision::Restart {
                after: Duration::from_secs(2)
            }
        );
    }

    #[test]
    fn backoff_is_capped_at_the_maximum() {
        let mut policy = RestartPolicy::with_limits(
            10,
            Duration::from_secs(60),
            Duration::from_secs(4),
            Duration::from_secs(10),
        );
        let start = Instant::now();

        assert_eq!(
            policy.on_exit(start),
            RestartDecision::Restart {
                after: Duration::from_secs(4)
            }
        );
        assert_eq!(
            policy.on_exit(start + Duration::from_millis(1)),
            RestartDecision::Restart {
                after: Duration::from_secs(8)
            }
        );
        assert_eq!(
            policy.on_exit(start + Duration::from_millis(2)),
            RestartDecision::Restart {
                after: Duration::from_secs(10)
            }
        );
    }

    #[test]
    fn exhausting_the_budget_inside_the_window_gives_up() {
        let mut policy = RestartPolicy::with_limits(
            2,
            Duration::from_secs(60),
            Duration::from_millis(500),
            Duration::from_secs(10),
        );
        let start = Instant::now();

        assert!(matches!(
            policy.on_exit(start),
            RestartDecision::Restart { .. }
        ));
        assert!(matches!(
            policy.on_exit(start + Duration::from_millis(1)),
            RestartDecision::Restart { .. }
        ));
        assert_eq!(
            policy.on_exit(start + Duration::from_millis(2)),
            RestartDecision::GiveUp
        );
    }

    #[test]
    fn failures_outside_the_window_never_accumulate() {
        let mut policy = RestartPolicy::with_limits(
            2,
            Duration::from_secs(1),
            Duration::from_millis(500),
            Duration::from_secs(10),
        );
        let start = Instant::now();

        for step in 0..6 {
            let now = start + Duration::from_secs(step * 2);
            assert!(
                matches!(policy.on_exit(now), RestartDecision::Restart { .. }),
                "isolated crashes spaced beyond the window must always restart"
            );
        }
    }
}
