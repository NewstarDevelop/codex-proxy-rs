fn client() -> reqwest::Client {
    use provider_xai::{GrokEndpointPolicy, OfficialGrokEndpointPolicy};
    OfficialGrokEndpointPolicy
        .build_inference_client(Some(std::time::Duration::from_secs(5)))
        .expect("production client")
}

include!("../../../../tests/proxy_contract.rs");
