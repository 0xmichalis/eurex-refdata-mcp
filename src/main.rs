//! Entry point: build the Eurex client and serve the MCP server over stdio.
//!
//! Optional environment variables (also loaded from a `.env` file in the working
//! directory):
//!   EUREX_API_KEY — personal API key from https://console.developer.deutsche-boerse.com/apis;
//!                   defaults to the shared anonymous key (rate-limited).

use eurex_refdata_mcp::eurex::transport::ReqwestTransport;
use eurex_refdata_mcp::eurex::EurexClient;
use eurex_refdata_mcp::mcp::EurexServer;
use rmcp::transport::stdio;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load a .env file if present; real environment variables take precedence.
    dotenvy::dotenv().ok();

    let mut client = EurexClient::new(ReqwestTransport::new()?);
    if let Ok(api_key) = std::env::var("EUREX_API_KEY") {
        if !api_key.is_empty() {
            client = client.with_api_key(api_key);
        }
    }
    let server = EurexServer::new(client);

    // stdout carries the MCP protocol; keep it clean (no logging to stdout).
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
