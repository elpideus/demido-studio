//! `LlamaCpp` against the contract suite.
//!
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): "Each
//! implementation's test file calls that function with itself. An
//! implementation that does not call it is not an implementation." This is that
//! file.
//!
//! It needs a card and a model, so it is `#[ignore]`d and run by the live
//! command in `AGENTS.md`. When it does run and the rig is absent it fails
//! rather than skipping.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

mod rig;

use std::time::Duration;

use demido_inference::{contract, Backend, LlamaCpp};

/// The whole suite, on the development model.
///
/// One tier rather than three: the contract is about the shape of the seam, not
/// about the weights behind it, and every case starts and stops a server. The
/// three tiers are what `a_real_model.rs` is for.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn llamacpp_keeps_the_contract() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let mut config = rig::require(rig::Tier::Development);
    config.context_length = contract::CONTEXT;

    contract::run::<LlamaCpp>(config, rig::Tier::Development.label()).await;
}

/// The half of "cancel leaves no orphaned process" that the contract cannot
/// ask, because it is about an operating system process and the trait does not
/// have one.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn stopping_leaves_no_llama_server_behind() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let config = rig::require(rig::Tier::Development);
    let backend = LlamaCpp::start(config).await.expect("started");
    let port = backend.port();

    assert!(
        listening(port).await,
        "the server it just started is not on the port it reported"
    );

    backend.stop().await;

    // Polled rather than asserted once: the process is killed, and the socket
    // it held is released by the operating system a moment afterwards.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while listening(port).await {
        assert!(
            std::time::Instant::now() < deadline,
            "port {port} is still held after stop, so the model is still in VRAM \
             with nothing left that can see it to offer to stop it"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// A backend that fails to start is reported and skipped rather than fatal.
///
/// The rig makes this cheap to state honestly: a real binary, a model path that
/// is not a model, so the failure is `llama.cpp`'s own rather than a check of
/// ours that ran first.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card; see the live command in AGENTS.md"]
async fn a_model_that_will_not_load_is_reported_with_the_servers_own_words() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let not_a_model = std::env::temp_dir().join("demido-not-a-model.gguf");
    tokio::fs::write(&not_a_model, b"this is not a GGUF")
        .await
        .expect("wrote the decoy");

    let mut config = rig::require(rig::Tier::Development);
    config.model = not_a_model.clone();
    config.startup_timeout = Duration::from_secs(60);

    let message = match LlamaCpp::start(config).await {
        Err(error) => error.to_string(),
        Ok(_) => panic!("llama.cpp loaded a file that is not a GGUF"),
    };

    let _ = tokio::fs::remove_file(&not_a_model).await;

    assert!(
        message.contains("It last said:"),
        "the server explained itself on stderr and the error must carry it, or \
         'inference did not start' is all anybody gets: {message}"
    );
}

async fn listening(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
}
