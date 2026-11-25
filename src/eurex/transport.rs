//! HTTP transport abstraction.
//!
//! The Eurex client depends on this trait rather than a concrete HTTP library, so the
//! query-building/parsing logic can be tested without network access (see
//! `tests/eurex_flow.rs`).

use std::time::Duration;

use reqwest::header::HeaderValue;

// Leave time for two sequential product search requests within the documented client timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
// Leave room for complete index options chains, which can span thousands of rows.
const RESPONSE_BYTES_MAX: usize = 16 * 1024 * 1024;

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
        Self::with_timeout(REQUEST_TIMEOUT)
    }

    fn with_timeout(timeout: Duration) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("eurex-refdata-mcp/", env!("CARGO_PKG_VERSION")))
            .timeout(timeout)
            .retry(reqwest::retry::never())
            // The credential belongs to the configured endpoint.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| TransportError(format!("building HTTP client: {}", e.without_url())))?;
        Ok(Self { client })
    }
}

impl PostGraphql for ReqwestTransport {
    async fn post(&self, url: &str, api_key: &str, body: &str) -> Result<String, TransportError> {
        let mut api_key = HeaderValue::from_str(api_key)
            .map_err(|_| TransportError("invalid API key header".into()))?;
        api_key.set_sensitive(true);
        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-DBP-APIKEY", api_key)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| request_error("sending request", e))?;
        let status = response.status();
        if !status.is_success() {
            return Err(TransportError(format!(
                "HTTP status error: {}",
                status.as_u16()
            )));
        }

        read_response(response, RESPONSE_BYTES_MAX).await
    }
}

async fn read_response(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<String, TransportError> {
    let too_large = || TransportError(format!("response body exceeds {limit} bytes"));
    let length = response.content_length();
    if length.is_some_and(|length| length > limit as u64) {
        return Err(too_large());
    }

    let mut body = Vec::with_capacity(length.unwrap_or(0) as usize);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| request_error("reading response body", e))?
    {
        if chunk.len() > limit - body.len() {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }

    // GraphQL JSON uses UTF-8. Tolerate the optional leading BOM.
    if body.starts_with(b"\xef\xbb\xbf") {
        body.drain(..3);
    }
    String::from_utf8(body).map_err(|_| TransportError("response body is not UTF-8".into()))
}

fn request_error(operation: &str, error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        return TransportError(format!("{operation}: deadline exceeded"));
    }
    TransportError(format!("{operation}: {}", error.without_url()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn reply(listener: TcpListener, response: &[u8]) -> TcpStream {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert_ne!(stream.read(&mut request).await.unwrap(), 0);
        stream.write_all(response).await.unwrap();
        // Keep the connection open to detect accidental reads past the limit.
        stream
    }

    async fn read_with_limit(raw_response: &[u8], limit: usize) -> Result<String, TransportError> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let transport = ReqwestTransport::with_timeout(Duration::from_secs(2)).unwrap();
        let read = async {
            let response = transport.client.post(url).send().await.unwrap();
            read_response(response, limit).await
        };
        let (result, _stream) = tokio::join!(read, reply(listener, raw_response));
        result
    }

    #[tokio::test]
    async fn known_oversized_body_is_rejected_before_reading_it() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n";
        let err = read_with_limit(response, 4).await.unwrap_err();
        assert_eq!(err, TransportError("response body exceeds 4 bytes".into()));
    }

    #[tokio::test]
    async fn chunked_body_limit_counts_total_bytes() {
        let response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n\"\xc3\r\n2\r\n\xa9\"\r\n0\r\n\r\n";
        assert_eq!(read_with_limit(response, 4).await.unwrap(), "\"é\"");
        let err = read_with_limit(response, 3).await.unwrap_err();
        assert_eq!(err, TransportError("response body exceeds 3 bytes".into()));
    }

    #[tokio::test]
    async fn accepts_a_utf8_bom_and_rejects_invalid_utf8() {
        let bom = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n\xef\xbb\xbf{}";
        assert_eq!(read_with_limit(bom, 5).await.unwrap(), "{}");
        let invalid = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n\"\xff\"";
        let err = read_with_limit(invalid, 3).await.unwrap_err();
        assert_eq!(err, TransportError("response body is not UTF-8".into()));
    }

    #[tokio::test]
    async fn deadline_applies_to_a_stalled_response_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let transport = ReqwestTransport::with_timeout(Duration::from_millis(250)).unwrap();
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{";
        let (result, _stream) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(
                transport.post(&url, "synthetic-key", "{}"),
                reply(listener, response),
            )
        })
        .await
        .expect("the transport deadline should fire first");
        assert_eq!(
            result.unwrap_err(),
            TransportError("reading response body: deadline exceeded".into())
        );
    }
}
