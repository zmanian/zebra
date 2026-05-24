//! Inbound service tests.

mod fake_peer_set;
mod real_peer_set;

use crate::BoxError;

/// Current behavior: score-bearing router errors from the semantic block verifier do not
/// downcast to `VerifyBlockError`, which matches the inbound cleanup path's skipped branch.
#[test]
fn score_bearing_router_error_does_not_downcast_to_verify_block_error() {
    use zebra_chain::block;
    use zebra_consensus::{BlockError, RouterError, VerifyBlockError};

    let block_hash = block::Hash([0; 32]);
    let verify_error = VerifyBlockError::Block {
        source: BlockError::MissingHeight(block_hash),
    };

    assert_eq!(verify_error.misbehavior_score(), 100);

    let router_error = RouterError::from(verify_error);
    assert_eq!(router_error.misbehavior_score(), 100);

    let boxed_error: BoxError = Box::new(router_error);
    let boxed_error = boxed_error
        .downcast::<VerifyBlockError>()
        .expect_err("inbound currently downcasts router errors as VerifyBlockError");

    let router_error = boxed_error
        .downcast::<RouterError>()
        .expect("the original score-bearing error is still a RouterError");
    assert_eq!(router_error.misbehavior_score(), 100);
}

#[test]
fn full_misbehavior_channel_drops_score_bearing_inbound_branch_today() {
    use zebra_chain::block;
    use zebra_consensus::{BlockError, VerifyBlockError};
    use zebra_network::PeerSocketAddr;

    let (misbehavior_sender, mut misbehavior_rx) = tokio::sync::mpsc::channel(1);
    let sentinel_addr: PeerSocketAddr = "127.0.0.1:8232"
        .parse()
        .expect("hard-coded peer address should parse");
    let advertiser_addr: PeerSocketAddr = "127.0.0.1:8233"
        .parse()
        .expect("hard-coded peer address should parse");

    misbehavior_sender
        .try_send((sentinel_addr, 1))
        .expect("test channel has one free slot");

    let block_hash = block::Hash([0; 32]);
    let verify_error = VerifyBlockError::Block {
        source: BlockError::MissingHeight(block_hash),
    };
    assert_eq!(verify_error.misbehavior_score(), 100);

    super::report_block_download_misbehavior(
        &misbehavior_sender,
        Box::new(verify_error),
        Some(advertiser_addr),
    );

    assert_eq!(
        misbehavior_rx
            .try_recv()
            .expect("sentinel report should still be queued"),
        (sentinel_addr, 1),
        "a full channel keeps the older report and drops the inbound branch report today"
    );
    assert!(
        matches!(
            misbehavior_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "the score-bearing inbound branch report is not queued when try_send() sees a full channel"
    );
}

#[tokio::test]
async fn inbound_router_error_score_is_not_reported_today() -> Result<(), BoxError> {
    use std::{iter, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

    use futures::{future::poll_fn, Stream};
    use tokio::{sync::oneshot, time::timeout};
    use tower::{buffer::Buffer, builder::ServiceBuilder, util::BoxService, Service};
    use tracing::Span;
    use zebra_chain::{
        block::{self, Height},
        parameters::Network,
        serialization::ZcashDeserializeInto,
    };
    use zebra_consensus::{BlockError, RouterError, VerifyBlockError};
    use zebra_network::{
        constants::{DEFAULT_MAX_CONNS_PER_IP, MAX_ADDRS_IN_ADDRESS_BOOK},
        AddressBook,
        InventoryResponse::Available,
        PeerSocketAddr,
    };
    use zebra_node_services::mempool;
    use zebra_state::Config as StateConfig;
    use zebra_test::mock_service::{MockService, PanicAssertion};

    use super::{downloads::MAX_INBOUND_CONCURRENCY, Inbound, InboundSetupData, Setup};

    let block: Arc<block::Block> =
        zebra_test::vectors::BLOCK_MAINNET_2_BYTES.zcash_deserialize_into()?;
    let block_hash = block.hash();
    let advertiser_addr: PeerSocketAddr = "127.0.0.1:8233"
        .parse()
        .expect("hard-coded peer address should parse");
    let network = Network::Mainnet;

    let address_book = AddressBook::new_with_addrs(
        SocketAddr::from_str("0.0.0.0:0").expect("hard-coded socket address should parse"),
        &network,
        DEFAULT_MAX_CONNS_PER_IP,
        MAX_ADDRS_IN_ADDRESS_BOOK,
        Span::none(),
        iter::empty(),
    );
    let address_book = Arc::new(std::sync::Mutex::new(address_book));

    let (state, _read_only_state_service, latest_chain_tip, _chain_tip_change) =
        zebra_state::init(StateConfig::ephemeral(), &network, Height::MAX, 0).await;
    let state_service = ServiceBuilder::new().buffer(1).service(state);

    let mut block_download_peer_set: MockService<_, _, PanicAssertion> = MockService::build()
        .with_max_request_delay(Duration::from_secs(5))
        .for_unit_tests();
    let buffered_block_download_peer_set =
        Buffer::new(BoxService::new(block_download_peer_set.clone()), 10);

    let mut block_verifier: MockService<
        zebra_consensus::Request,
        block::Hash,
        PanicAssertion,
        RouterError,
    > = MockService::build()
        .with_max_request_delay(Duration::from_secs(5))
        .for_unit_tests();
    let buffered_block_verifier = Buffer::new(BoxService::new(block_verifier.clone()), 1);

    let mempool_service: MockService<mempool::Request, mempool::Response, PanicAssertion> =
        MockService::build().for_unit_tests();
    let buffered_mempool_service = Buffer::new(BoxService::new(mempool_service), 1);

    let (misbehavior_sender, mut misbehavior_rx) = tokio::sync::mpsc::channel(1);
    let (setup_tx, setup_rx) = oneshot::channel();
    let mut inbound = Inbound::new(MAX_INBOUND_CONCURRENCY, setup_rx);

    let setup_result = setup_tx.send(InboundSetupData {
        address_book,
        block_download_peer_set: buffered_block_download_peer_set,
        block_verifier: buffered_block_verifier,
        mempool: buffered_mempool_service,
        state: state_service,
        latest_chain_tip,
        misbehavior_sender,
    });
    assert!(
        setup_result.is_ok(),
        "inbound setup receiver should be live"
    );

    poll_fn(|cx| inbound.poll_ready(cx))
        .await
        .expect("inbound setup should initialize successfully");

    let response = inbound
        .call(zebra_network::Request::AdvertiseBlock(
            block_hash,
            Some(advertiser_addr),
        ))
        .await
        .expect("advertised block hashes are queued without synchronous verification");
    assert_eq!(response, zebra_network::Response::Nil);

    block_download_peer_set
        .expect_request(zebra_network::Request::BlocksByHash(
            std::iter::once(block_hash).collect(),
        ))
        .await
        .respond(zebra_network::Response::Blocks(vec![Available((
            block.clone(),
            Some(advertiser_addr),
        ))]));

    let verify_error = VerifyBlockError::Block {
        source: BlockError::MissingHeight(block_hash),
    };
    assert_eq!(verify_error.misbehavior_score(), 100);
    let router_error = RouterError::from(verify_error);
    assert_eq!(router_error.misbehavior_score(), 100);

    block_verifier
        .expect_request(zebra_consensus::Request::Commit(block))
        .await
        .respond_error(router_error);

    timeout(Duration::from_secs(5), async {
        loop {
            poll_fn(|cx| inbound.poll_ready(cx))
                .await
                .expect("inbound cleanup polling should succeed");

            match &inbound.setup {
                Setup::Initialized {
                    block_downloads, ..
                } if block_downloads.size_hint().0 == 0 => break,
                Setup::Initialized { .. } => tokio::task::yield_now().await,
                _ => panic!("inbound should remain initialized after setup"),
            }
        }
    })
    .await
    .expect("inbound cleanup should drain the failed block download");

    assert!(
        matches!(
            misbehavior_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "score-bearing RouterError from inbound gossiped block verification is not reported today"
    );

    Ok(())
}
