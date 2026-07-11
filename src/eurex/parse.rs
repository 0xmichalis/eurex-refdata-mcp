//! Parsing of GraphQL response envelopes into typed rows.
//!
//! Every Eurex query returns `{"data": {"<Root>": {"date": ..., "data": [...]}}}` on
//! success and an `errors` array on failure (possibly alongside partial data).

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

/// Errors surfaced by the Eurex client.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EurexError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("Eurex API error: {0}")]
    Api(String),
    #[error("unexpected response: {0}")]
    Parse(String),
}

/// One option instrument of a chain, as returned by the `Contracts` query.
/// Every field is optional because GraphQL fields are nullable.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct OptionContract {
    #[serde(
        rename(deserialize = "Product"),
        skip_serializing_if = "Option::is_none"
    )]
    pub product: Option<String>,
    #[serde(
        rename(deserialize = "Contract"),
        skip_serializing_if = "Option::is_none"
    )]
    pub contract: Option<String>,
    #[serde(
        rename(deserialize = "ContractID"),
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_id: Option<i64>,
    #[serde(
        rename(deserialize = "InstrumentID"),
        skip_serializing_if = "Option::is_none"
    )]
    pub instrument_id: Option<i64>,
    #[serde(rename(deserialize = "ISIN"), skip_serializing_if = "Option::is_none")]
    pub isin: Option<String>,
    #[serde(
        rename(deserialize = "CallPut"),
        skip_serializing_if = "Option::is_none"
    )]
    pub call_put: Option<String>,
    #[serde(
        rename(deserialize = "Strike"),
        skip_serializing_if = "Option::is_none"
    )]
    pub strike: Option<f64>,
    #[serde(
        rename(deserialize = "ExpirationDate"),
        skip_serializing_if = "Option::is_none"
    )]
    pub expiration_date: Option<String>,
    #[serde(
        rename(deserialize = "LastTradingDate"),
        skip_serializing_if = "Option::is_none"
    )]
    pub last_trading_date: Option<String>,
    #[serde(
        rename(deserialize = "ContractSize"),
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_size: Option<f64>,
    #[serde(
        rename(deserialize = "ExerciseStyle"),
        skip_serializing_if = "Option::is_none"
    )]
    pub exercise_style: Option<String>,
    #[serde(
        rename(deserialize = "SettlementType"),
        skip_serializing_if = "Option::is_none"
    )]
    pub settlement_type: Option<String>,
    #[serde(
        rename(deserialize = "PreviousDaySettlementPrice"),
        skip_serializing_if = "Option::is_none"
    )]
    pub previous_day_settlement_price: Option<f64>,
    #[serde(
        rename(deserialize = "OptionsDelta"),
        skip_serializing_if = "Option::is_none"
    )]
    pub options_delta: Option<f64>,
}

/// One product, as returned by the `ProductInfos` query.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct ProductInfo {
    #[serde(
        rename(deserialize = "Product"),
        skip_serializing_if = "Option::is_none"
    )]
    pub product: Option<String>,
    #[serde(rename(deserialize = "Name"), skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(
        rename(deserialize = "ProductLine"),
        skip_serializing_if = "Option::is_none"
    )]
    pub product_line: Option<String>,
    #[serde(
        rename(deserialize = "ProductType"),
        skip_serializing_if = "Option::is_none"
    )]
    pub product_type: Option<String>,
    #[serde(
        rename(deserialize = "Currency"),
        skip_serializing_if = "Option::is_none"
    )]
    pub currency: Option<String>,
    #[serde(
        rename(deserialize = "Underlying"),
        skip_serializing_if = "Option::is_none"
    )]
    pub underlying: Option<String>,
    #[serde(
        rename(deserialize = "UnderlyingName"),
        skip_serializing_if = "Option::is_none"
    )]
    pub underlying_name: Option<String>,
    #[serde(
        rename(deserialize = "UnderlyingISIN"),
        skip_serializing_if = "Option::is_none"
    )]
    pub underlying_isin: Option<String>,
    #[serde(
        rename(deserialize = "ContractSize"),
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_size: Option<f64>,
    #[serde(
        rename(deserialize = "TickSize"),
        skip_serializing_if = "Option::is_none"
    )]
    pub tick_size: Option<f64>,
    #[serde(
        rename(deserialize = "TickValue"),
        skip_serializing_if = "Option::is_none"
    )]
    pub tick_value: Option<f64>,
}

/// One expiration of a product, as returned by the `Expirations` query.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct Expiration {
    #[serde(
        rename(deserialize = "Product"),
        skip_serializing_if = "Option::is_none"
    )]
    pub product: Option<String>,
    #[serde(
        rename(deserialize = "ExpirationDate"),
        skip_serializing_if = "Option::is_none"
    )]
    pub expiration_date: Option<String>,
    #[serde(
        rename(deserialize = "LastTradingDate"),
        skip_serializing_if = "Option::is_none"
    )]
    pub last_trading_date: Option<String>,
}

/// Extract the row list under `data.<root>.data` from a GraphQL response body.
/// GraphQL errors are reported as [`EurexError::Api`] with all messages joined.
pub fn parse_rows<T: DeserializeOwned>(body: &str, root: &str) -> Result<Vec<T>, EurexError> {
    let envelope: Value =
        serde_json::from_str(body).map_err(|e| EurexError::Parse(format!("invalid JSON: {e}")))?;

    if let Some(errors) = envelope.get("errors").and_then(Value::as_array) {
        let messages: Vec<&str> = errors
            .iter()
            .filter_map(|e| e.get("message").and_then(Value::as_str))
            .collect();
        return Err(EurexError::Api(messages.join("; ")));
    }

    let rows = envelope
        .get("data")
        .and_then(|d| d.get(root))
        .and_then(|r| r.get("data"))
        .cloned()
        .ok_or_else(|| EurexError::Parse(format!("missing data.{root}.data")))?;

    serde_json::from_value(rows).map_err(|e| EurexError::Parse(format!("decoding rows: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_contract_rows() {
        let body = r#"{"data":{"Contracts":{"date":"2026-07-13","data":[
            {"Product":"ALV","Contract":"ALV SI 20261218 PS AM C 430.00 0","CallPut":"C",
             "Strike":430,"ExpirationDate":"2026-12-18","ISIN":"DE000F3KP6F6",
             "PreviousDaySettlementPrice":19.57,"OptionsDelta":0.5}]}}}"#;

        let rows: Vec<OptionContract> = parse_rows(body, "Contracts").unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].strike, Some(430.0));
        assert_eq!(rows[0].call_put.as_deref(), Some("C"));
        assert_eq!(rows[0].contract_id, None);
    }

    #[test]
    fn empty_data_list_is_ok_and_empty() {
        let body = r#"{"data":{"ProductInfos":{"data":[]}}}"#;
        let rows: Vec<ProductInfo> = parse_rows(body, "ProductInfos").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn graphql_errors_map_to_api_error_with_joined_messages() {
        let body = r#"{"errors":[{"message":"Cannot query field \"X\""},{"message":"boom"}]}"#;

        let err = parse_rows::<OptionContract>(body, "Contracts").unwrap_err();

        assert_eq!(
            err,
            EurexError::Api("Cannot query field \"X\"; boom".into())
        );
    }

    #[test]
    fn invalid_json_is_a_parse_error() {
        let err = parse_rows::<OptionContract>("not json", "Contracts").unwrap_err();
        assert!(matches!(err, EurexError::Parse(_)));
    }

    #[test]
    fn missing_root_is_a_parse_error_naming_the_path() {
        let body = r#"{"data":{"Something":{}}}"#;
        let err = parse_rows::<OptionContract>(body, "Contracts").unwrap_err();
        assert_eq!(err, EurexError::Parse("missing data.Contracts.data".into()));
    }
}
