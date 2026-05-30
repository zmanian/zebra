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
    assert_eq!(
        c.stall_restart_timeout,
        std::time::Duration::from_secs(120)
    );
}
