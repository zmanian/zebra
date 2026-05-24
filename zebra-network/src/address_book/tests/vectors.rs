//! Fixed test vectors for the address book.

use std::time::{Duration, Instant};

use chrono::Utc;
use tracing::Span;

use zebra_chain::{
    parameters::Network::*,
    serialization::{DateTime32, Duration32},
};

use crate::{
    address_book_updater::AddressBookUpdater,
    constants::{
        DEFAULT_MAX_CONNS_PER_IP, MAX_ADDRS_IN_ADDRESS_BOOK, MAX_PEER_MISBEHAVIOR_SCORE,
        MIN_PEER_RECONNECTION_DELAY,
    },
    meta_addr::{MetaAddr, MetaAddrChange},
    protocol::external::types::PeerServices,
    AddressBook, Config,
};

/// Make sure an empty address book is actually empty.
#[test]
fn address_book_empty() {
    let address_book = AddressBook::new(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        Span::current(),
    );

    assert_eq!(
        address_book
            .reconnection_peers(Instant::now(), Utc::now())
            .next(),
        None
    );
    assert_eq!(address_book.len(), 0);
}

#[test]
#[should_panic(expected = "should be some when should_remove_most_recent_by_ip is true")]
fn misbehavior_ban_panics_with_max_connections_per_ip_above_one_today() {
    let mut address_book =
        AddressBook::new("0.0.0.0:0".parse().unwrap(), &Mainnet, 2, Span::current());

    address_book.update(MetaAddrChange::UpdateMisbehavior {
        addr: "127.0.0.1:8233".parse().unwrap(),
        score_increment: MAX_PEER_MISBEHAVIOR_SCORE,
    });
}

#[tokio::test]
async fn misbehavior_ban_panics_updater_and_poisons_address_book_today() {
    let _init_guard = zebra_test::init();

    let config = Config {
        max_connections_per_ip: 2,
        ..Config::default()
    };
    let (
        address_book,
        _bans_receiver,
        address_book_updater,
        _address_metrics,
        address_book_updater_task,
    ) = AddressBookUpdater::spawn(&config, config.listen_addr);

    address_book_updater
        .send(MetaAddrChange::UpdateMisbehavior {
            addr: "127.0.0.1:8233".parse().unwrap(),
            score_increment: MAX_PEER_MISBEHAVIOR_SCORE,
        })
        .await
        .expect("updater receiver should still be live before the panic");

    let join_result = address_book_updater_task.await;
    assert!(
        join_result
            .expect_err("ban-threshold misbehavior should panic the updater task")
            .is_panic(),
        "updater task should exit by panic today"
    );
    assert!(
        address_book.lock().is_err(),
        "panic while holding the address-book mutex should poison it today"
    );
}

#[test]
fn ban_cleanup_leaves_non_contiguous_same_ip_entries_today() {
    let banned_addr1 = "127.0.0.1:8233".parse().unwrap();
    let other_addr = "127.0.0.2:8233".parse().unwrap();
    let banned_addr2 = "127.0.0.1:8234".parse().unwrap();

    let banned_meta_addr1 =
        MetaAddr::new_gossiped_meta_addr(banned_addr1, PeerServices::NODE_NETWORK, DateTime32::MIN);
    let other_meta_addr = MetaAddr::new_gossiped_meta_addr(
        other_addr,
        PeerServices::NODE_NETWORK,
        DateTime32::MIN.saturating_add(Duration32::from_seconds(1)),
    );
    let banned_meta_addr2 = MetaAddr::new_gossiped_meta_addr(
        banned_addr2,
        PeerServices::NODE_NETWORK,
        DateTime32::MIN.saturating_add(Duration32::from_seconds(2)),
    );

    let mut address_book = AddressBook::new_with_addrs(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::current(),
        [banned_meta_addr1, other_meta_addr, banned_meta_addr2],
    );

    address_book.update(MetaAddrChange::UpdateMisbehavior {
        addr: banned_addr1,
        score_increment: MAX_PEER_MISBEHAVIOR_SCORE,
    });

    assert!(
        address_book.bans().contains_key(&banned_addr1.ip()),
        "the misbehavior update should ban the shared IP"
    );
    assert!(
        address_book.get(banned_addr1).is_some() || address_book.get(banned_addr2).is_some(),
        "current ban cleanup leaves at least one non-contiguous banned-IP entry in the address book"
    );
    assert!(
        address_book.get(other_addr).is_some(),
        "unrelated IP entries should remain after banning a different IP"
    );
}

#[test]
fn inbound_ephemeral_address_becomes_reconnection_candidate_today() {
    let mut address_book = AddressBook::new(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        Span::current(),
    );

    let inbound_ephemeral_addr = "198.51.100.10:49152".parse().unwrap();
    let updated = address_book
        .update(MetaAddr::new_connected(
            inbound_ephemeral_addr,
            &PeerServices::NODE_NETWORK,
            true,
        ))
        .expect("inbound remote address is accepted into the address book today");

    assert!(updated.is_inbound());

    let later = MIN_PEER_RECONNECTION_DELAY + Duration::from_secs(1);
    let later_chrono =
        Utc::now() + chrono::Duration::from_std(later).expect("test duration fits in chrono");

    assert_eq!(
        address_book
            .reconnection_peers(Instant::now() + later, later_chrono)
            .next()
            .map(|peer| peer.addr()),
        Some(inbound_ephemeral_addr),
    );
}

/// Make sure peers are attempted in priority order.
#[test]
fn address_book_peer_order() {
    let addr1 = "127.0.0.1:1".parse().unwrap();
    let addr2 = "127.0.0.2:2".parse().unwrap();

    let mut meta_addr1 =
        MetaAddr::new_gossiped_meta_addr(addr1, PeerServices::NODE_NETWORK, DateTime32::MIN);
    let mut meta_addr2 = MetaAddr::new_gossiped_meta_addr(
        addr2,
        PeerServices::NODE_NETWORK,
        DateTime32::MIN.saturating_add(Duration32::from_seconds(1)),
    );

    // Regardless of the order of insertion, the most recent address should be chosen first
    let addrs = vec![meta_addr1, meta_addr2];
    let address_book = AddressBook::new_with_addrs(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::current(),
        addrs,
    );
    assert_eq!(
        address_book
            .reconnection_peers(Instant::now(), Utc::now())
            .next(),
        Some(meta_addr2),
    );

    // Reverse the order, check that we get the same result
    let addrs = vec![meta_addr2, meta_addr1];
    let address_book = AddressBook::new_with_addrs(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::current(),
        addrs,
    );
    assert_eq!(
        address_book
            .reconnection_peers(Instant::now(), Utc::now())
            .next(),
        Some(meta_addr2),
    );

    // Now check that the order depends on the time, not the address
    meta_addr1.addr = addr2;
    meta_addr2.addr = addr1;

    let addrs = vec![meta_addr1, meta_addr2];
    let address_book = AddressBook::new_with_addrs(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::current(),
        addrs,
    );
    assert_eq!(
        address_book
            .reconnection_peers(Instant::now(), Utc::now())
            .next(),
        Some(meta_addr2),
    );

    // Reverse the order, check that we get the same result
    let addrs = vec![meta_addr2, meta_addr1];
    let address_book = AddressBook::new_with_addrs(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::current(),
        addrs,
    );
    assert_eq!(
        address_book
            .reconnection_peers(Instant::now(), Utc::now())
            .next(),
        Some(meta_addr2),
    );
}

/// Check that `reconnection_peers` skips addresses with IPs for which
/// Zebra already has recently updated outbound peers.
#[test]
fn reconnection_peers_skips_recently_updated_ip() {
    // tests that reconnection_peers() skips addresses where there's a connection at that IP with a recent:
    // - `last_response`
    test_reconnection_peers_skips_recently_updated_ip(true, |addr| {
        MetaAddr::new_responded(addr, None)
    });

    // tests that reconnection_peers() *does not* skip addresses where there's a connection at that IP with a recent:
    // - `last_attempt`
    test_reconnection_peers_skips_recently_updated_ip(false, MetaAddr::new_reconnect);
    // - `last_failure`
    test_reconnection_peers_skips_recently_updated_ip(false, |addr| {
        MetaAddr::new_errored(addr, PeerServices::NODE_NETWORK)
    });
}

fn test_reconnection_peers_skips_recently_updated_ip<
    M: Fn(crate::PeerSocketAddr) -> crate::meta_addr::MetaAddrChange,
>(
    should_skip_ip: bool,
    make_meta_addr_change: M,
) {
    let addr1 = "127.0.0.1:1".parse().unwrap();
    let addr2 = "127.0.0.1:2".parse().unwrap();

    let meta_addr1 = make_meta_addr_change(addr1).into_new_meta_addr(
        Instant::now(),
        Utc::now().try_into().expect("will succeed until 2038"),
    );
    let meta_addr2 = MetaAddr::new_gossiped_meta_addr(
        addr2,
        PeerServices::NODE_NETWORK,
        DateTime32::MIN.saturating_add(Duration32::from_seconds(1)),
    );

    // The second address should be skipped because the first address has a
    // recent `last_response` time and the two addresses have the same IP.
    let addrs = vec![meta_addr1, meta_addr2];
    let address_book = AddressBook::new_with_addrs(
        "0.0.0.0:0".parse().unwrap(),
        &Mainnet,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::current(),
        addrs,
    );

    let next_reconnection_peer = address_book
        .reconnection_peers(Instant::now(), Utc::now())
        .next();

    if should_skip_ip {
        assert_eq!(next_reconnection_peer, None,);
    } else {
        assert_ne!(next_reconnection_peer, None,);
    }
}
