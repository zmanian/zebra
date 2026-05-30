//! Config tests for the sync component.

#[test]
fn stall_restart_timeout_default_is_90s() {
    let _init_guard = zebra_test::init();

    assert_eq!(
        super::super::Config::default().stall_restart_timeout,
        std::time::Duration::from_secs(90)
    );
}

#[test]
fn stall_restart_timeout_disabled_with_zero() {
    let _init_guard = zebra_test::init();

    let c: super::super::Config = toml::from_str("stall_restart_timeout = \"0s\"").unwrap();
    assert!(c.stall_restart_timeout.is_zero());
}

#[test]
fn stall_restart_timeout_round_trips() {
    let _init_guard = zebra_test::init();

    let c: super::super::Config = toml::from_str("stall_restart_timeout = \"120s\"").unwrap();
    assert_eq!(c.stall_restart_timeout, std::time::Duration::from_secs(120));
}

#[test]
fn clamp_stall_restart_timeout_below_verify_timeout_passes_through() {
    let _init_guard = zebra_test::init();

    let configured = std::time::Duration::from_secs(90);
    assert!(configured < super::super::BLOCK_VERIFY_TIMEOUT);
    assert_eq!(
        super::super::clamp_stall_restart_timeout(configured),
        configured,
        "a value below the block verify timeout should pass through unchanged",
    );
}

#[test]
fn clamp_stall_restart_timeout_zero_stays_disabled() {
    let _init_guard = zebra_test::init();

    assert_eq!(
        super::super::clamp_stall_restart_timeout(std::time::Duration::ZERO),
        std::time::Duration::ZERO,
        "zero (disabled) should pass through unchanged",
    );
}

#[test]
fn clamp_stall_restart_timeout_at_or_above_verify_timeout_is_halved() {
    let _init_guard = zebra_test::init();

    let expected = super::super::BLOCK_VERIFY_TIMEOUT / 2;
    assert_eq!(
        super::super::clamp_stall_restart_timeout(super::super::BLOCK_VERIFY_TIMEOUT),
        expected,
        "a value equal to the block verify timeout should be clamped to half of it",
    );
    assert_eq!(
        super::super::clamp_stall_restart_timeout(super::super::BLOCK_VERIFY_TIMEOUT * 2),
        expected,
        "a value above the block verify timeout should be clamped to half of it",
    );
}
