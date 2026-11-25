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
async fn rate_limit_is_reported_once_without_sensitive_request_details() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("X-DBP-APIKEY", "header-sentinel"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let url = format!("{}/?key=url-sentinel", server.uri());
    let err = transport
        .post(&url, "header-sentinel", "{}")
        .await
        .unwrap_err();

    assert_eq!(err.to_string(), "HTTP status error: 429");
}

#[tokio::test]
async fn request_errors_omit_the_url() {
    let transport = ReqwestTransport::new().unwrap();
    let err = transport
        .post("file:///url-sentinel", "header-sentinel", "{}")
        .await
        .unwrap_err();
    assert!(err.to_string().starts_with("sending request:"));
    assert!(!format!("{err:?} {err}").contains("sentinel"));
}

#[tokio::test]
async fn a_redirect_does_not_forward_the_api_key() {
    let server = MockServer::start().await;
    let destination = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", destination.uri()))
        .expect(1)
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let err = transport
        .post(&server.uri(), "header-sentinel", "{}")
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "HTTP status error: 307");
    assert!(destination.received_requests().await.unwrap().is_empty());
}
