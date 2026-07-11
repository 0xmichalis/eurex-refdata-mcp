//! GraphQL request bodies for the Eurex Reference Data API.
//!
//! Filter values are always passed as GraphQL variables, never interpolated into the
//! query document, so user-supplied strings cannot change the query shape.

use serde_json::{json, Map, Value};

/// Narrowing criteria for an options chain. `product` is the Eurex product code
/// (e.g. "ALV"); everything else is optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChainFilter {
    pub product: String,
    /// Exact expiration date, `YYYY-MM-DD`.
    pub expiration_date: Option<String>,
    /// "C" for calls, "P" for puts.
    pub call_put: Option<String>,
    pub min_strike: Option<f64>,
    pub max_strike: Option<f64>,
}

const CONTRACTS_QUERY: &str = "query Chain($filter: ContractsFilter!) { \
     Contracts(filter: $filter) { date data { \
     Product Contract ContractID InstrumentID ISIN CallPut Strike \
     ExpirationDate LastTradingDate ContractSize ExerciseStyle SettlementType \
     PreviousDaySettlementPrice OptionsDelta } } }";

const PRODUCT_INFOS_QUERY: &str = "query Products($filter: ProductInfosFilter) { \
     ProductInfos(filter: $filter) { date data { \
     Product Name ProductLine ProductType Currency Underlying UnderlyingName \
     UnderlyingISIN ContractSize TickSize TickValue } } }";

const EXPIRATIONS_QUERY: &str = "query Expirations($filter: ExpirationsFilter) { \
     Expirations(filter: $filter) { date data { \
     Product ExpirationDate LastTradingDate } } }";

fn body(query: &str, filter: Value) -> String {
    json!({ "query": query, "variables": { "filter": filter } }).to_string()
}

/// Contracts query for an options chain: the product's option instruments
/// (`ProductLine = "O"`), optionally narrowed by expiry, side, and strike range.
pub fn contracts_body(filter: &ChainFilter) -> String {
    let mut f = Map::new();
    f.insert("Product".into(), json!({ "eq": filter.product }));
    f.insert("ProductLine".into(), json!({ "eq": "O" }));
    if let Some(date) = &filter.expiration_date {
        f.insert("ExpirationDate".into(), json!({ "eq": date }));
    }
    if let Some(side) = &filter.call_put {
        f.insert("CallPut".into(), json!({ "eq": side }));
    }
    let mut strike = Map::new();
    if let Some(min) = filter.min_strike {
        strike.insert("ge".into(), json!(min));
    }
    if let Some(max) = filter.max_strike {
        strike.insert("le".into(), json!(max));
    }
    if !strike.is_empty() {
        f.insert("Strike".into(), Value::Object(strike));
    }
    body(CONTRACTS_QUERY, Value::Object(f))
}

/// ProductInfos query matching an exact underlying symbol (e.g. "ALV").
pub fn products_by_underlying_body(symbol: &str) -> String {
    body(
        PRODUCT_INFOS_QUERY,
        json!({ "Underlying": { "eq": symbol } }),
    )
}

/// ProductInfos query matching a fragment of the underlying's long name
/// (e.g. "ALLIANZ"). Names are stored uppercase.
pub fn products_by_underlying_name_body(fragment: &str) -> String {
    body(
        PRODUCT_INFOS_QUERY,
        json!({ "UnderlyingName": { "contains": fragment } }),
    )
}

/// Expirations query for one product.
pub fn expirations_body(product: &str) -> String {
    body(EXPIRATIONS_QUERY, json!({ "Product": { "eq": product } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter_of(body: &str) -> Value {
        let v: Value = serde_json::from_str(body).unwrap();
        assert!(v["query"].as_str().unwrap().starts_with("query "));
        v["variables"]["filter"].clone()
    }

    #[test]
    fn chain_with_only_product_filters_product_and_option_line() {
        let body = contracts_body(&ChainFilter {
            product: "ALV".into(),
            ..Default::default()
        });

        let filter = filter_of(&body);
        assert_eq!(
            filter,
            json!({ "Product": { "eq": "ALV" }, "ProductLine": { "eq": "O" } })
        );
    }

    #[test]
    fn chain_with_all_criteria_builds_full_filter() {
        let body = contracts_body(&ChainFilter {
            product: "ALV".into(),
            expiration_date: Some("2026-12-18".into()),
            call_put: Some("C".into()),
            min_strike: Some(400.0),
            max_strike: Some(440.0),
        });

        let filter = filter_of(&body);
        assert_eq!(filter["ExpirationDate"], json!({ "eq": "2026-12-18" }));
        assert_eq!(filter["CallPut"], json!({ "eq": "C" }));
        assert_eq!(filter["Strike"], json!({ "ge": 400.0, "le": 440.0 }));
    }

    #[test]
    fn chain_with_only_min_strike_omits_le() {
        let body = contracts_body(&ChainFilter {
            product: "ALV".into(),
            min_strike: Some(400.0),
            ..Default::default()
        });

        assert_eq!(filter_of(&body)["Strike"], json!({ "ge": 400.0 }));
    }

    #[test]
    fn product_value_is_a_variable_not_interpolated_into_the_query() {
        let body = contracts_body(&ChainFilter {
            product: "\"}) { evil }".into(),
            ..Default::default()
        });

        let v: Value = serde_json::from_str(&body).unwrap();
        assert!(!v["query"].as_str().unwrap().contains("evil"));
        assert_eq!(v["variables"]["filter"]["Product"]["eq"], "\"}) { evil }");
    }

    #[test]
    fn product_searches_filter_by_symbol_and_name() {
        let by_symbol = filter_of(&products_by_underlying_body("ALV"));
        assert_eq!(by_symbol, json!({ "Underlying": { "eq": "ALV" } }));

        let by_name = filter_of(&products_by_underlying_name_body("ALLIANZ"));
        assert_eq!(
            by_name,
            json!({ "UnderlyingName": { "contains": "ALLIANZ" } })
        );
    }

    #[test]
    fn expirations_filter_by_product() {
        let filter = filter_of(&expirations_body("ALV"));
        assert_eq!(filter, json!({ "Product": { "eq": "ALV" } }));
    }
}
