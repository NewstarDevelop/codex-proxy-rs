fn client() -> reqwest::Client {
    provider_openai::transport::build_reqwest_client().expect("production client")
}

include!("../../../../tests/proxy_contract.rs");

#[test]
fn websocket_proxy_environment_contract() {
    run_proxy_contract("websocket_proxy_child");
}

#[tokio::test]
async fn websocket_proxy_child() {
    if std::env::var("CPR_PROXY_TEST_MODE").is_err() {
        return;
    }
    provider_openai::transport::tls::ensure_rustls_provider();
    let target = std::env::var("CPR_PROXY_TEST_TARGET")
        .unwrap()
        .replacen("https://", "wss://", 1);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(target),
        )
        .await
        .expect("bounded proxy handshake")
        .is_err()
    );
}
