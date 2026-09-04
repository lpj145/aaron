use aaron_core::{BackoffStrategy, BoxError, RestartPolicy, ServiceOpts};
use std::time::Duration;

#[test]
fn test_backoff_none() {
    let backoff = BackoffStrategy::none();
    assert_eq!(backoff.calculate_delay(0), Duration::ZERO);
    assert_eq!(backoff.calculate_delay(5), Duration::ZERO);
}

#[test]
fn test_backoff_constant() {
    let backoff = BackoffStrategy::constant(Duration::from_secs(2));
    assert_eq!(backoff.calculate_delay(0), Duration::from_secs(2));
    assert_eq!(backoff.calculate_delay(10), Duration::from_secs(2));
}

#[test]
fn test_backoff_linear() {
    let backoff = BackoffStrategy::linear(
        Duration::from_secs(1),
        Duration::from_secs(2),
        Some(Duration::from_secs(6)),
    );
    assert_eq!(backoff.calculate_delay(0), Duration::from_secs(1)); // 1 + 0*2 = 1
    assert_eq!(backoff.calculate_delay(1), Duration::from_secs(3)); // 1 + 1*2 = 3
    assert_eq!(backoff.calculate_delay(2), Duration::from_secs(5)); // 1 + 2*2 = 5
    assert_eq!(backoff.calculate_delay(3), Duration::from_secs(6)); // 1 + 3*2 = 7 -> capped at 6
}

#[test]
fn test_backoff_exponential() {
    let backoff = BackoffStrategy::exponential(
        Duration::from_millis(100),
        Some(Duration::from_millis(1000)),
        2.0,
    );
    assert_eq!(backoff.calculate_delay(0), Duration::from_millis(100)); // 100 * 2^0 = 100
    assert_eq!(backoff.calculate_delay(1), Duration::from_millis(200)); // 100 * 2^1 = 200
    assert_eq!(backoff.calculate_delay(2), Duration::from_millis(400)); // 100 * 2^2 = 400
    assert_eq!(backoff.calculate_delay(3), Duration::from_millis(800)); // 100 * 2^3 = 800
    assert_eq!(backoff.calculate_delay(4), Duration::from_millis(1000)); // 100 * 2^4 = 1600 -> capped at 1000
}

#[test]
fn test_restart_policy_helpers() {
    let never = RestartPolicy::never();
    assert!(never.is_never());
    assert!(!never.is_always());
    assert!(!never.is_on_failure());

    let always = RestartPolicy::always();
    assert!(!always.is_never());
    assert!(always.is_always());
    assert!(!always.is_on_failure());

    let on_failure = RestartPolicy::on_failure();
    assert!(!on_failure.is_never());
    assert!(!on_failure.is_always());
    assert!(on_failure.is_on_failure());

    let max_retries = RestartPolicy::on_failure_max_retries(3);
    assert!(max_retries.is_on_failure());
}

#[test]
fn test_restart_policy_should_restart() {
    let ok_result: Result<(), BoxError> = Ok(());
    let err_result: Result<(), BoxError> = Err("some error".into());

    // Default (Never)
    let opts = ServiceOpts::default();
    assert!(!opts.should_restart(&ok_result, 0));
    assert!(!opts.should_restart(&err_result, 0));

    // RestartPolicy::Always
    let opts = ServiceOpts::new().restart_always();
    assert!(opts.should_restart(&ok_result, 0));
    assert!(opts.should_restart(&err_result, 0));
    assert!(opts.should_restart(&err_result, 100));

    // RestartPolicy::OnFailure
    let opts = ServiceOpts::new().restart_on_failure();
    assert!(!opts.should_restart(&ok_result, 0));
    assert!(opts.should_restart(&err_result, 0));
    assert!(opts.should_restart(&err_result, 50));

    // RestartPolicy::OnFailureMaxRetries
    let opts = ServiceOpts::new().on_failure_max_retries(3);
    assert!(!opts.should_restart(&ok_result, 0));
    assert!(opts.should_restart(&err_result, 0));
    assert!(opts.should_restart(&err_result, 1));
    assert!(opts.should_restart(&err_result, 2));
    assert!(!opts.should_restart(&err_result, 3));

    // RestartPolicy::MaxRetries also restarts on successful completion, up to the limit
    let opts = ServiceOpts::new().max_retries(2);
    assert!(opts.should_restart(&ok_result, 0));
    assert!(opts.should_restart(&ok_result, 1));
    assert!(!opts.should_restart(&ok_result, 2));
}
