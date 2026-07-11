//! The functional core: a transport-agnostic client for the Eurex Reference Data
//! GraphQL API (read-only, anonymous access via a shared API key).
//!
//! Docs: <https://www.eurex.com/ex-en/data/free-reference-data-api>

mod parse;
pub mod query;
pub mod transport;

pub use parse::{EurexError, Expiration, OptionContract, ProductInfo};
pub use query::ChainFilter;

use parse::parse_rows;
use transport::PostGraphql;

/// Production GraphQL endpoint. The trailing slash is required — without it the
/// gateway returns 404.
pub const API_URL: &str = "https://api.developer.deutsche-boerse.com/eurex-prod-graphql/";

/// Shared anonymous API key published on the Eurex reference data page (rate-limited).
/// A personal key from <https://console.developer.deutsche-boerse.com/apis> lifts the limit.
pub const PUBLIC_API_KEY: &str = "68cdafd2-c5c1-49be-8558-37244ab4f513";

/// Runs Eurex reference data queries over an injected [`PostGraphql`] transport.
pub struct EurexClient<T: PostGraphql> {
    transport: T,
    url: String,
    api_key: String,
}

impl<T: PostGraphql> EurexClient<T> {
    /// Create a client against the production endpoint with the shared anonymous key.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            url: API_URL.to_string(),
            api_key: PUBLIC_API_KEY.to_string(),
        }
    }

    /// Override the endpoint (used by tests to point at a mock server).
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Use a personal API key instead of the shared anonymous one.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = api_key.into();
        self
    }

    /// Fetch the options chain for a product, sorted by expiration, then strike,
    /// then call/put.
    pub async fn options_chain(
        &self,
        filter: &ChainFilter,
    ) -> Result<Vec<OptionContract>, EurexError> {
        let body = self.post(&query::contracts_body(filter)).await?;
        let mut rows: Vec<OptionContract> = parse_rows(&body, "Contracts")?;
        rows.sort_by(|a, b| {
            a.expiration_date
                .cmp(&b.expiration_date)
                .then_with(|| {
                    a.strike
                        .unwrap_or(f64::MAX)
                        .total_cmp(&b.strike.unwrap_or(f64::MAX))
                })
                .then_with(|| a.call_put.cmp(&b.call_put))
        });
        Ok(rows)
    }

    /// Find products by underlying: matches the exact underlying symbol and, for the
    /// uppercased query, fragments of the underlying's long name. Results are merged,
    /// deduplicated by product code, and sorted.
    pub async fn search_products(&self, search: &str) -> Result<Vec<ProductInfo>, EurexError> {
        let upper = search.to_uppercase();
        let by_symbol = self
            .post(&query::products_by_underlying_body(&upper))
            .await?;
        let by_name = self
            .post(&query::products_by_underlying_name_body(&upper))
            .await?;

        let mut rows: Vec<ProductInfo> = parse_rows(&by_symbol, "ProductInfos")?;
        rows.extend(parse_rows::<ProductInfo>(&by_name, "ProductInfos")?);
        rows.sort_by(|a, b| a.product.cmp(&b.product));
        rows.dedup_by(|a, b| a.product == b.product);
        Ok(rows)
    }

    /// Fetch a product's expirations, deduplicated by date and sorted.
    pub async fn expirations(&self, product: &str) -> Result<Vec<Expiration>, EurexError> {
        let body = self.post(&query::expirations_body(product)).await?;
        let mut rows: Vec<Expiration> = parse_rows(&body, "Expirations")?;
        rows.sort_by(|a, b| a.expiration_date.cmp(&b.expiration_date));
        rows.dedup_by(|a, b| a.expiration_date == b.expiration_date);
        Ok(rows)
    }

    async fn post(&self, body: &str) -> Result<String, EurexError> {
        self.transport
            .post(&self.url, &self.api_key, body)
            .await
            .map_err(|e| EurexError::Transport(e.to_string()))
    }
}
