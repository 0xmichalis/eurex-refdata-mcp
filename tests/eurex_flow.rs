//! End-to-end flow tests for the Eurex client using an injected fake transport — no network.

use eurex_refdata_mcp::eurex::transport::{PostGraphql, TransportError};
use eurex_refdata_mcp::eurex::{ChainFilter, EurexClient, EurexError, API_URL, PUBLIC_API_KEY};
use std::collections::VecDeque;
use std::sync::Mutex;

/// A recorded request: URL, API key, and the JSON body.
type RecordedCall = (String, String, String);

/// Records every request and replays a queued sequence of response bodies.
struct FakeTransport {
    responses: Mutex<VecDeque<String>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl FakeTransport {
    fn new(responses: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(String::from).collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl PostGraphql for FakeTransport {
    async fn post(&self, url: &str, api_key: &str, body: &str) -> Result<String, TransportError> {
        self.calls
            .lock()
            .unwrap()
            .push((url.to_string(), api_key.to_string(), body.to_string()));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| TransportError("fake: no more responses queued".into()))
    }
}

const CHAIN: &str = r#"{"data":{"Contracts":{"date":"2026-07-13","data":[
    {"Product":"ALV","Contract":"ALV C2","CallPut":"C","Strike":440,"ExpirationDate":"2026-12-18"},
    {"Product":"ALV","Contract":"ALV C1","CallPut":"C","Strike":400,"ExpirationDate":"2026-12-18"},
    {"Product":"ALV","Contract":"ALV C0","CallPut":"C","Strike":420,"ExpirationDate":"2026-08-21"}]}}}"#;

#[tokio::test]
async fn options_chain_is_sorted_by_expiry_then_strike() {
    let client = EurexClient::new(FakeTransport::new([CHAIN]));

    let chain = client
        .options_chain(&ChainFilter {
            product: "ALV".into(),
            ..Default::default()
        })
        .await
        .unwrap();

    let order: Vec<(&str, f64)> = chain
        .iter()
        .map(|c| (c.expiration_date.as_deref().unwrap(), c.strike.unwrap()))
        .collect();
    assert_eq!(
        order,
        vec![
            ("2026-08-21", 420.0),
            ("2026-12-18", 400.0),
            ("2026-12-18", 440.0),
        ]
    );
}

#[tokio::test]
async fn posts_to_production_url_with_shared_key_by_default() {
    let fake = FakeTransport::new([CHAIN]);
    let client = EurexClient::new(&fake);

    client
        .options_chain(&ChainFilter {
            product: "ALV".into(),
            ..Default::default()
        })
        .await
        .unwrap();

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let (url, api_key, body) = &calls[0];
    assert_eq!(url, API_URL);
    assert_eq!(api_key, PUBLIC_API_KEY);
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(parsed["variables"]["filter"]["Product"]["eq"], "ALV");
}

#[tokio::test]
async fn search_products_merges_symbol_and_name_matches_deduplicated() {
    let by_symbol = r#"{"data":{"ProductInfos":{"data":[
        {"Product":"ALV","Name":"OPT ON ALLIANZ SE","Underlying":"ALV"}]}}}"#;
    let by_name = r#"{"data":{"ProductInfos":{"data":[
        {"Product":"ALV","Name":"OPT ON ALLIANZ SE","Underlying":"ALV"},
        {"Product":"ALVG","Name":"FUT ON ALLIANZ SE","Underlying":"ALV"}]}}}"#;
    let fake = FakeTransport::new([by_symbol, by_name]);
    let client = EurexClient::new(&fake);

    let products = client.search_products("allianz").await.unwrap();

    let codes: Vec<&str> = products
        .iter()
        .map(|p| p.product.as_deref().unwrap())
        .collect();
    assert_eq!(codes, vec!["ALV", "ALVG"]);

    // The query is uppercased for both searches (Eurex stores names uppercase).
    for (_, _, body) in fake.calls() {
        assert!(body.contains("ALLIANZ"), "body: {body}");
        assert!(!body.contains("allianz"), "body: {body}");
    }
}

#[tokio::test]
async fn expirations_are_deduplicated_and_sorted() {
    let body = r#"{"data":{"Expirations":{"data":[
        {"Product":"ALV","ExpirationDate":"2027-12-17","LastTradingDate":"2027-12-17"},
        {"Product":"ALV","ExpirationDate":"2026-07-17","LastTradingDate":"2026-07-17"},
        {"Product":"ALV","ExpirationDate":"2026-07-17","LastTradingDate":"2026-07-17"}]}}}"#;
    let client = EurexClient::new(FakeTransport::new([body]));

    let expirations = client.expirations("ALV").await.unwrap();

    let dates: Vec<&str> = expirations
        .iter()
        .map(|e| e.expiration_date.as_deref().unwrap())
        .collect();
    assert_eq!(dates, vec!["2026-07-17", "2027-12-17"]);
}

#[tokio::test]
async fn graphql_error_response_surfaces_as_api_error() {
    let client = EurexClient::new(FakeTransport::new([
        r#"{"errors":[{"message":"rate limit exceeded"}]}"#,
    ]));

    let err = client.expirations("ALV").await.unwrap_err();

    assert_eq!(err, EurexError::Api("rate limit exceeded".into()));
}

#[tokio::test]
async fn transport_failure_surfaces_as_transport_error() {
    let client = EurexClient::new(FakeTransport::new([]));

    let err = client.expirations("ALV").await.unwrap_err();

    assert!(matches!(err, EurexError::Transport(_)));
}
