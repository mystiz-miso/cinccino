use std::time::Duration;

use cinccino::server::{CinccinoBackend, SlowRequestLayer};
use tower_lsp::{LspService, Server};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// LSP requests taking longer than this are logged at WARN with the
/// request name and elapsed time. Mirrors the 10 s threshold used by
/// the indexer's LspClient, so both ends of the wire agree on "slow".
const SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() {
    // Tracing goes to stderr so the JSON-RPC stdout channel stays pure.
    // Default is WARN for the visible output; `RUST_LOG=cinccino=debug`
    // shows per-request DEBUG spans with elapsed time (FmtSpan::CLOSE).
    //
    // The EnvFilter is attached PER-LAYER (via `.with_filter`) rather
    // than to the Registry as a whole. A registry-level filter disables
    // span creation entirely for filtered levels, which would prevent
    // SlowRequestLayer from ever observing DEBUG handler spans — the
    // slow-request WARN would then be dead code in prod. Per-layer
    // filtering lets fmt stay quiet while SlowRequestLayer still sees
    // every span and can escalate slow ones on its own.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_target(true)
        .with_filter(env_filter);
    let slow_layer = SlowRequestLayer::new(SLOW_REQUEST_THRESHOLD)
        // Slow-request logging cares about every span down to DEBUG
        // regardless of what fmt is showing.
        .with_filter(LevelFilter::DEBUG);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(slow_layer)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(CinccinoBackend::new);

    Server::new(stdin, stdout, socket).serve(service).await;
}
