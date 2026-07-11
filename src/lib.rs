//! Read-only Eurex Reference Data client and MCP server.
//!
//! The [`eurex`] module is the functional core: a transport-agnostic client for the
//! Deutsche Börse Eurex Reference Data GraphQL API (products, options chains,
//! expirations). It is read-only by construction — the API serves reference data only.

pub mod eurex;
pub mod mcp;
