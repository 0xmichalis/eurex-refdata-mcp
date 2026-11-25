//! MCP server exposing the Eurex reference data tools over stdio (via `rmcp`).
//!
//! Three read-only tools: `eurex_search_products` (find the product code for an
//! underlying), `eurex_expirations` (a product's expiry dates), and
//! `eurex_options_chain` (the option contracts of a product). The upstream API is
//! reference data only — there is nothing here that can trade.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use tokio::sync::Semaphore;

use crate::eurex::transport::ReqwestTransport;
use crate::eurex::{ChainFilter, EurexClient, EurexError};

// Keep simultaneous response reads and result construction bounded in one stdio session.
const ACTIVE_QUERIES_MAX: usize = 4;

/// The MCP server: an Eurex client over the production transport.
pub struct EurexServer {
    client: EurexClient<ReqwestTransport>,
    active_queries: Semaphore,
}

impl EurexServer {
    pub fn new(client: EurexClient<ReqwestTransport>) -> Self {
        Self {
            client,
            active_queries: Semaphore::new(ACTIVE_QUERIES_MAX),
        }
    }

    async fn run_query<T: serde::Serialize>(
        &self,
        query: impl Future<Output = Result<Vec<T>, EurexError>>,
        key: &str,
    ) -> CallToolResult {
        let Ok(_permit) = self.active_queries.try_acquire() else {
            return CallToolResult::error(vec![ContentBlock::text(
                "Eurex server is busy. Retry when another query finishes.",
            )]);
        };
        to_result(query.await, key)
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchProductsArgs {
    /// Underlying symbol (e.g. "ALV") or a fragment of the underlying's name
    /// (e.g. "Allianz"). Case-insensitive.
    pub query: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExpirationsArgs {
    /// Eurex product code (e.g. "ALV" for Allianz options, "OESX" for EURO STOXX 50
    /// options). Use eurex_search_products to find it.
    pub product: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OptionsChainArgs {
    /// Eurex product code (e.g. "ALV", "OESX"). Use eurex_search_products to find it.
    pub product: String,
    /// Exact expiration date, YYYY-MM-DD. Use eurex_expirations to list valid dates.
    pub expiration_date: Option<String>,
    /// "C" for calls, "P" for puts. Omit for both.
    pub call_put: Option<String>,
    /// Lowest strike to include.
    pub min_strike: Option<f64>,
    /// Highest strike to include.
    pub max_strike: Option<f64>,
}

impl OptionsChainArgs {
    /// Validate and normalize the tool input before querying Eurex.
    fn into_filter(self) -> Result<ChainFilter, String> {
        let call_put = match self.call_put.as_deref() {
            None => None,
            Some("C" | "c") => Some("C".into()),
            Some("P" | "p") => Some("P".into()),
            Some(_) => return Err("call_put must be \"C\" or \"P\"".into()),
        };
        if let (Some(min), Some(max)) = (self.min_strike, self.max_strike)
            && min > max
        {
            return Err("min_strike must be less than or equal to max_strike".into());
        }
        Ok(ChainFilter {
            product: self.product,
            expiration_date: self.expiration_date,
            call_put,
            min_strike: self.min_strike,
            max_strike: self.max_strike,
        })
    }
}

#[tool_router]
impl EurexServer {
    #[tool(
        name = "eurex_search_products",
        description = "Find Eurex products (options, futures) by underlying symbol or name, \
                       e.g. 'ALV' or 'Allianz'. Returns product codes to use with the other \
                       tools, with product line, type, currency, contract size, and tick data. \
                       Read-only reference data.",
        annotations(read_only_hint = true)
    )]
    async fn search_products(
        &self,
        Parameters(args): Parameters<SearchProductsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(self
            .run_query(self.client.search_products(&args.query), "products")
            .await)
    }

    #[tool(
        name = "eurex_expirations",
        description = "List the expiration dates of a Eurex product (e.g. ALV, OESX), sorted \
                       ascending. Useful to narrow an options chain query. Read-only reference \
                       data.",
        annotations(read_only_hint = true)
    )]
    async fn expirations(
        &self,
        Parameters(args): Parameters<ExpirationsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(self
            .run_query(self.client.expirations(&args.product), "expirations")
            .await)
    }

    #[tool(
        name = "eurex_options_chain",
        description = "Fetch the options chain of a Eurex product (e.g. ALV, OESX): strikes, \
                       expiries, ISINs, latest daily settlement prices and previous-day deltas, \
                       sorted by expiry then strike. Chains can exceed 1000 contracts — narrow with \
                       expiration_date, call_put, and min/max_strike. Read-only reference data.",
        annotations(read_only_hint = true)
    )]
    async fn options_chain(
        &self,
        Parameters(args): Parameters<OptionsChainArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let filter = match args.into_filter() {
            Ok(filter) => filter,
            Err(message) => return Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
        };
        Ok(self
            .run_query(self.client.options_chain(&filter), "contracts")
            .await)
    }
}

#[tool_handler]
impl ServerHandler for EurexServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Read-only Eurex reference data (Deutsche Börse GraphQL API). Typical flow: \
                 `eurex_search_products` to find a product code from an underlying, \
                 `eurex_expirations` to pick an expiry, then `eurex_options_chain` filtered \
                 by that expiry. Settlement prices are the most recent daily settlement and \
                 deltas are previous-day values; neither is a live quote.",
            )
    }
}

/// Map a client outcome to an MCP tool result. A failure is reported as a tool-level
/// error (`is_error = true`) so the model sees the message, rather than a protocol error.
fn to_result<T: serde::Serialize>(result: Result<Vec<T>, EurexError>, key: &str) -> CallToolResult {
    match result {
        Ok(rows) => CallToolResult::structured(serde_json::json!({ key: rows })),
        Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
            "Eurex query failed: {err}"
        ))]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eurex::OptionContract;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn contract(strike: f64) -> OptionContract {
        serde_json::from_value(serde_json::json!({ "Strike": strike, "CallPut": "C" })).unwrap()
    }

    #[test]
    fn rows_map_to_structured_json_under_the_given_key() {
        let result = to_result(Ok(vec![contract(430.0)]), "contracts");

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("structuredContent"), "json: {json}");
        assert!(json.contains("\"strike\":430.0"), "json: {json}");
    }

    #[test]
    fn no_rows_yield_empty_list_not_error() {
        let result = to_result(Ok(Vec::<OptionContract>::new()), "contracts");

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"contracts\":[]"), "json: {json}");
    }

    #[test]
    fn client_error_maps_to_tool_error_with_message() {
        let result = to_result::<OptionContract>(Err(EurexError::Api("boom".into())), "contracts");

        assert_eq!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("boom"), "json: {json}");
    }

    #[test]
    fn call_put_is_normalized_to_uppercase() {
        let args = OptionsChainArgs {
            product: "ALV".into(),
            expiration_date: None,
            call_put: Some("p".into()),
            min_strike: None,
            max_strike: None,
        };

        assert_eq!(args.into_filter().unwrap().call_put.as_deref(), Some("P"));
    }

    #[test]
    fn invalid_call_put_is_rejected() {
        let args = OptionsChainArgs {
            product: "ALV".into(),
            expiration_date: None,
            call_put: Some("call".into()),
            min_strike: None,
            max_strike: None,
        };

        let err = args.into_filter().unwrap_err();
        assert!(err.contains("call_put"), "err: {err}");
    }

    #[tokio::test]
    async fn chain_tool_rejects_inverted_strikes_and_accepts_equal_strikes() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"data":{"Contracts":{"data":[{"Strike":400}]}}}"#),
            )
            .expect(1)
            .mount(&mock)
            .await;
        let client = EurexClient::new(ReqwestTransport::new().unwrap())
            .with_url(mock.uri())
            .with_api_key("synthetic-key");
        let server = EurexServer::new(client);
        let args = |max_strike| {
            Parameters(OptionsChainArgs {
                product: "TEST".into(),
                expiration_date: None,
                call_put: None,
                min_strike: Some(400.0),
                max_strike: Some(max_strike),
            })
        };

        let result = server.options_chain(args(399.0)).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(mock.received_requests().await.unwrap().is_empty());

        let result = server.options_chain(args(400.0)).await.unwrap();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result.structured_content.unwrap(),
            serde_json::json!({"contracts":[{"strike":400.0}]}),
        );
    }

    #[tokio::test]
    async fn tool_calls_fail_fast_when_busy_and_resume_when_capacity_frees() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"data":{"Expirations":{"data":[]}}}"#),
            )
            .expect(1)
            .mount(&mock)
            .await;
        let client = EurexClient::new(ReqwestTransport::new().unwrap())
            .with_url(mock.uri())
            .with_api_key("synthetic-key");
        let server = EurexServer::new(client);
        let args = || {
            Parameters(ExpirationsArgs {
                product: "TEST".into(),
            })
        };
        let permits = server
            .active_queries
            .try_acquire_many(ACTIVE_QUERIES_MAX as u32)
            .unwrap();

        let result = server.expirations(args()).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(mock.received_requests().await.unwrap().is_empty());

        drop(permits);
        let result = server.expirations(args()).await.unwrap();
        assert_eq!(result.is_error, Some(false));
    }
}
