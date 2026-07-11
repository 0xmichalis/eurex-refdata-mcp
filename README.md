# eurex-refdata-mcp

Read-only MCP server for the [Eurex Reference Data API](https://www.eurex.com/ex-en/data/free-reference-data-api) (Deutsche Börse GraphQL API). Fetch options chains, product reference data, and expirations from Eurex — no credentials required.

## Tools

| Tool | Description |
|------|-------------|
| `eurex_search_products` | Find products by underlying symbol or name (e.g. `ALV`, `Allianz`) |
| `eurex_expirations` | List a product's expiration dates |
| `eurex_options_chain` | Fetch an options chain, filterable by expiry, call/put, and strike range |

Prices in the chain (`previous_day_settlement_price`, `options_delta`) are previous-day settlement data, not live quotes.

## Install

Prebuilt **static** Linux binaries are attached to each [GitHub Release](../../releases) —
built for `x86_64-unknown-linux-musl`, so they link no libc and run on any x86_64 Linux
regardless of the host's glibc version.

```sh
tar xzf eurex-refdata-mcp-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz
install eurex-refdata-mcp-vX.Y.Z-x86_64-unknown-linux-musl/eurex-refdata-mcp ~/.local/bin/
```

Or build from source: `cargo build --release` (a static musl build uses
[`cross`](https://github.com/cross-rs/cross): `cross build --release --target x86_64-unknown-linux-musl`).

## Setup

No credentials required: the server defaults to the shared anonymous key published by Eurex, which is rate-limited. For dedicated throughput, create a personal key at the [Deutsche Börse developer portal](https://console.developer.deutsche-boerse.com/apis) and provide it as `EUREX_API_KEY`, either as an environment variable or in a `.env` file in the working directory (loaded via dotenvy; real environment variables take precedence). A `.env` is gitignored.

## Use with a Hermes agent

Point your `~/.hermes/config.yaml` at the binary. If you use a personal API key, keep it in
`~/.hermes/.env` and reference it with `${VAR}` (Hermes interpolates from `~/.hermes/.env`):

```yaml
mcp_servers:
  eurex:
    command: /home/you/.local/bin/eurex-refdata-mcp
    args: []
    env:
      EUREX_API_KEY: "${EUREX_API_KEY}"  # optional; omit to use the shared key
    timeout: 120
    connect_timeout: 60
```

```sh
# ~/.hermes/.env  (chmod 600)
EUREX_API_KEY=your-key
```

Verify: `hermes mcp test eurex` should connect and list the three tools.

## Development

```bash
cargo test                                        # unit + integration (no network)
cargo test --test live_eurex -- --ignored         # live tests against the real API
```

Pre-commit hooks (fmt, clippy, check, test) are installed by `cargo-husky` on the first `cargo test`.

## License

MIT
