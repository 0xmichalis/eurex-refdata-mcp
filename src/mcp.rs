//! MCP server exposing the Eurex reference data tools over stdio (via `rmcp`).
//!
//! Three read-only tools: `eurex_search_products` (find the product code for an
//! underlying), `eurex_expirations` (a product's expiry dates), and
//! `eurex_options_chain` (the option contracts of a product). The upstream API is
//! reference data only — there is nothing here that can trade.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::eurex::transport::ReqwestTransport;
use crate::eurex::{ChainFilter, EurexClient, EurexError};

/// The MCP server: an Eurex client over the production transport.
pub struct EurexServer {
    client: EurexClient<ReqwestTransport>,
}

impl EurexServer {
    pub fn new(client: EurexClient<ReqwestTransport>) -> Self {
        Self { client }
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
    /// Validate and normalize into a client-side filter. Rejects a call/put flag that
    /// is not C or P — the API would silently return an empty chain otherwise.
    fn into_filter(self) -> Result<ChainFilter, String> {
        let call_put = match self.call_put.as_deref().map(str::to_uppercase) {
            None => None,
            Some(cp) if cp == "C" || cp == "P" => Some(cp),
            Some(other) => return Err(format!("call_put must be \"C\" or \"P\", got {other:?}")),
        };
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
        Ok(to_result(
            self.client.search_products(&args.query).await,
            "products",
        ))
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
        Ok(to_result(
            self.client.expirations(&args.product).await,
            "expirations",
        ))
    }

    #[tool(
        name = "eurex_options_chain",
        description = "Fetch the options chain of a Eurex product (e.g. ALV, OESX): strikes, \
                       expiries, ISINs, previous-day settlement prices and deltas, sorted by \
                       expiry then strike. Chains can exceed 1000 contracts — narrow with \
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
        Ok(to_result(
            self.client.options_chain(&filter).await,
            "contracts",
        ))
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
                 by that expiry. Settlement prices are previous-day, not live quotes.",
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
}
