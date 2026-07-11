//! HTTP transport abstraction.
//!
//! The Eurex client depends on this trait rather than a concrete HTTP library, so the
//! query-building/parsing logic can be tested without network access (see
//! `tests/eurex_flow.rs`).

/// An error from the underlying HTTP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TransportError {}

/// Performs an HTTP POST of a JSON body (a GraphQL request), returning the response body
/// as text. `api_key` is sent as the `X-DBP-APIKEY` header.
#[allow(async_fn_in_trait)]
pub trait PostGraphql {
    async fn post(&self, url: &str, api_key: &str, body: &str) -> Result<String, TransportError>;
}

/// Allow a shared reference to a transport to be used as a transport, so a single transport
/// can be shared (e.g. handed to a client while still inspectable in tests).
impl<T: PostGraphql> PostGraphql for &T {
    async fn post(&self, url: &str, api_key: &str, body: &str) -> Result<String, TransportError> {
        (**self).post(url, api_key, body).await
    }
}

/// Production transport backed by `reqwest` with rustls TLS.
#[derive(Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Build a transport with a fixed user agent and the default rustls TLS stack.
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("eurex-refdata-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| TransportError(format!("building HTTP client: {e}")))?;
        Ok(Self { client })
    }
}

impl PostGraphql for ReqwestTransport {
    async fn post(&self, url: &str, api_key: &str, body: &str) -> Result<String, TransportError> {
        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-DBP-APIKEY", api_key)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| TransportError(format!("request failed: {e}")))?
            .error_for_status()
            .map_err(|e| TransportError(format!("HTTP status error: {e}")))?;
        response
            .text()
            .await
            .map_err(|e| TransportError(format!("reading response body: {e}")))
    }
}
