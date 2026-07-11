//! Tests for the real reqwest-backed transport, against a local mock HTTP server.

use eurex_refdata_mcp::eurex::transport::{PostGraphql, ReqwestTransport};
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn post_sends_json_body_and_api_key_header_and_returns_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql/"))
        .and(header("X-DBP-APIKEY", "my-key"))
        .and(header("Content-Type", "application/json"))
        .and(body_string(r#"{"query":"{__typename}"}"#))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":{}}"#))
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let url = format!("{}/graphql/", server.uri());
    let body = transport
        .post(&url, "my-key", r#"{"query":"{__typename}"}"#)
        .await
        .unwrap();

    assert_eq!(body, r#"{"data":{}}"#);
}

#[tokio::test]
async fn non_success_status_is_a_transport_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let err = transport.post(&server.uri(), "k", "{}").await.unwrap_err();

    assert!(err.to_string().contains("429"), "error was: {err}");
}
