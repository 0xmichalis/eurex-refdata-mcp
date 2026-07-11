//! Live integration tests against the real Eurex Reference Data API.
//!
//! The API is anonymous (shared, rate-limited key), so these tests need no credentials —
//! but they hit the network, so they are `#[ignore]`d by default. Run them with:
//!
//!     cargo test --test live_eurex -- --ignored --nocapture

use eurex_refdata_mcp::eurex::transport::ReqwestTransport;
use eurex_refdata_mcp::eurex::{ChainFilter, EurexClient};

fn live_client() -> EurexClient<ReqwestTransport> {
    let _ = dotenvy::dotenv();
    let mut client = EurexClient::new(ReqwestTransport::new().expect("build reqwest transport"));
    if let Ok(api_key) = std::env::var("EUREX_API_KEY") {
        if !api_key.is_empty() {
            client = client.with_api_key(api_key);
        }
    }
    client
}

#[tokio::test]
#[ignore = "hits the live Eurex API"]
async fn live_allianz_flow_products_expirations_chain() {
    let client = live_client();

    let products = client.search_products("Allianz").await.unwrap();
    println!("products: {}", products.len());
    assert!(
        products.iter().any(|p| p.product.as_deref() == Some("ALV")),
        "expected the ALV option product among: {:?}",
        products
            .iter()
            .map(|p| p.product.clone())
            .collect::<Vec<_>>()
    );

    let expirations = client.expirations("ALV").await.unwrap();
    println!("expirations: {}", expirations.len());
    let expiry = expirations
        .first()
        .and_then(|e| e.expiration_date.clone())
        .expect("ALV should have at least one expiration");

    let chain = client
        .options_chain(&ChainFilter {
            product: "ALV".into(),
            expiration_date: Some(expiry.clone()),
            call_put: Some("C".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    println!("chain for {expiry}: {} calls", chain.len());
    assert!(!chain.is_empty(), "expected calls for expiry {expiry}");
    assert!(chain.iter().all(|c| c.call_put.as_deref() == Some("C")));
    assert!(chain
        .iter()
        .all(|c| c.expiration_date.as_deref() == Some(expiry.as_str())));
    for c in chain.iter().take(5) {
        println!(
            "  {} strike={:?} settle={:?} delta={:?}",
            c.contract.as_deref().unwrap_or("-"),
            c.strike,
            c.previous_day_settlement_price,
            c.options_delta,
        );
    }
}
