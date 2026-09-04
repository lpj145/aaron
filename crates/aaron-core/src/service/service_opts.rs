use super::BoxError;
use std::time::Duration;

/// Strategy to define delay before restarting a service.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum BackoffStrategy {
    /// No delay between restarts.
    #[default]
    None,
    /// Constant delay between restarts.
    Constant(Duration),
    /// Linear delay incrementing by `step` for each attempt: `initial + (step * retry_count)`.
    Linear {
        initial: Duration,
        step: Duration,
        max: Option<Duration>,
    },
    /// Exponential delay multiplying by `multiplier` for each attempt: `initial * multiplier^retry_count`.
    Exponential {
        initial: Duration,
        max: Option<Duration>,
        multiplier: f64,
    },
}

impl BackoffStrategy {
    /// Creates a backoff strategy with no delay.
    pub fn none() -> Self {
        Self::None
    }

    /// Creates a constant backoff strategy.
    pub fn constant(delay: Duration) -> Self {
        Self::Constant(delay)
    }

    /// Creates a linear backoff strategy.
    pub fn linear(initial: Duration, step: Duration, max: Option<Duration>) -> Self {
        Self::Linear { initial, step, max }
    }

    /// Creates an exponential backoff strategy.
    pub fn exponential(initial: Duration, max: Option<Duration>, multiplier: f64) -> Self {
        Self::Exponential {
            initial,
            max,
            multiplier,
        }
    }

    /// Calculates the delay for a specific retry attempt (0-indexed: 0 is the 1st retry, 1 is the 2nd, etc.).
    pub fn calculate_delay(&self, retry_count: u32) -> Duration {
        match self {
            Self::None => Duration::ZERO,
            Self::Constant(duration) => *duration,
            Self::Linear { initial, step, max } => {
                let delay = initial.saturating_add(step.saturating_mul(retry_count));
                if let Some(max_delay) = max {
                    delay.min(*max_delay)
                } else {
                    delay
                }
            }
            Self::Exponential {
                initial,
                max,
                multiplier,
            } => {
                if multiplier.is_nan() {
                    return max.unwrap_or(Duration::MAX);
                }
                let mult = multiplier.powf(f64::from(retry_count));
                let secs = initial.as_secs_f64() * mult;
                let delay = if secs >= 0.0 {
                    Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
                } else {
                    Duration::MAX
                };

                if let Some(max_delay) = max {
                    delay.min(*max_delay)
                } else {
                    delay
                }
            }
        }
    }
}

/// Restart policy defining under what conditions a service should be restarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartPolicy {
    /// Never restart the service.
    #[default]
    Never,
    /// Always restart the service, regardless of exit status.
    Always,
    /// Restart only when the service exits with an error.
    OnFailure,
    /// Restart up to `max` retries.
    MaxRetries(u32),
    /// Restart on failure up to `max` retries.
    OnFailureMaxRetries(u32),
}

impl RestartPolicy {
    /// Creates a `RestartPolicy::Never`.
    pub fn never() -> Self {
        Self::Never
    }

    /// Creates a `RestartPolicy::Always`.
    pub fn always() -> Self {
        Self::Always
    }

    /// Creates a `RestartPolicy::OnFailure`.
    pub fn on_failure() -> Self {
        Self::OnFailure
    }

    /// Creates a `RestartPolicy::MaxRetries`.
    pub fn max_retries(max: u32) -> Self {
        Self::MaxRetries(max)
    }

    /// Creates a `RestartPolicy::OnFailureMaxRetries`.
    pub fn on_failure_max_retries(max: u32) -> Self {
        Self::OnFailureMaxRetries(max)
    }

    /// Returns `true` if the policy is `Never`.
    pub fn is_never(&self) -> bool {
        matches!(self, Self::Never)
    }

    /// Returns `true` if the policy is `Always`.
    pub fn is_always(&self) -> bool {
        matches!(self, Self::Always)
    }

    /// Returns `true` if the policy is configured to restart on failure.
    pub fn is_on_failure(&self) -> bool {
        matches!(self, Self::OnFailure | Self::OnFailureMaxRetries(_))
    }
}

/// Options and configuration for running a service within a Node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServiceOpts {
    /// Backoff strategy for delay between restarts.
    pub backoff: BackoffStrategy,
    /// Restart policy determining when restarts occur.
    pub restart_policy: RestartPolicy,
}

impl ServiceOpts {
    /// Creates a new `ServiceOpts` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the backoff strategy.
    pub fn backoff(mut self, backoff: BackoffStrategy) -> Self {
        self.backoff = backoff;
        self
    }

    /// Sets a constant backoff delay.
    pub fn constant_backoff(mut self, delay: Duration) -> Self {
        self.backoff = BackoffStrategy::Constant(delay);
        self
    }

    /// Sets a linear backoff strategy.
    pub fn linear_backoff(
        mut self,
        initial: Duration,
        step: Duration,
        max: Option<Duration>,
    ) -> Self {
        self.backoff = BackoffStrategy::Linear { initial, step, max };
        self
    }

    /// Sets an exponential backoff strategy.
    pub fn exponential_backoff(
        mut self,
        initial: Duration,
        max: Option<Duration>,
        multiplier: f64,
    ) -> Self {
        self.backoff = BackoffStrategy::Exponential {
            initial,
            max,
            multiplier,
        };
        self
    }

    /// Sets the restart policy.
    pub fn restart_policy(mut self, restart_policy: RestartPolicy) -> Self {
        self.restart_policy = restart_policy;
        self
    }

    /// Sets restart policy to `RestartPolicy::Always`.
    pub fn restart_always(mut self) -> Self {
        self.restart_policy = RestartPolicy::Always;
        self
    }

    /// Sets restart policy to `RestartPolicy::OnFailure`.
    pub fn restart_on_failure(mut self) -> Self {
        self.restart_policy = RestartPolicy::OnFailure;
        self
    }

    /// Sets restart policy to `RestartPolicy::Never`.
    pub fn restart_never(mut self) -> Self {
        self.restart_policy = RestartPolicy::Never;
        self
    }

    /// Sets restart policy to `RestartPolicy::MaxRetries(max)`.
    pub fn max_retries(mut self, max: u32) -> Self {
        self.restart_policy = RestartPolicy::MaxRetries(max);
        self
    }

    /// Sets restart policy to `RestartPolicy::OnFailureMaxRetries(max)`.
    pub fn on_failure_max_retries(mut self, max: u32) -> Self {
        self.restart_policy = RestartPolicy::OnFailureMaxRetries(max);
        self
    }

    /// Determines if a service should restart given the outcome of its previous run and the number of retries already performed.
    ///
    /// The decision is driven entirely by `restart_policy` — its variants already say
    /// whether success counts (`Always`, `MaxRetries`) or only failure does (`OnFailure`,
    /// `OnFailureMaxRetries`, `Never`), so there's no separate "restart on completion" switch.
    pub fn should_restart(&self, result: &Result<(), BoxError>, retry_count: u32) -> bool {
        match result {
            Ok(()) => match self.restart_policy {
                RestartPolicy::Always => true,
                RestartPolicy::MaxRetries(max) => retry_count < max,
                RestartPolicy::Never
                | RestartPolicy::OnFailure
                | RestartPolicy::OnFailureMaxRetries(_) => false,
            },
            Err(_) => match self.restart_policy {
                RestartPolicy::Never => false,
                RestartPolicy::Always | RestartPolicy::OnFailure => true,
                RestartPolicy::MaxRetries(max) | RestartPolicy::OnFailureMaxRetries(max) => {
                    retry_count < max
                }
            },
        }
    }

    /// Calculates the delay before the next retry (0-indexed retry count).
    pub fn retry_delay(&self, retry_count: u32) -> Duration {
        self.backoff.calculate_delay(retry_count)
    }
}
