//! log_probe: emits three structured tracing events and exits. Used by
//! tests/logging_smoke.rs to verify JSON format end-to-end. Not shipped in
//! release artifacts (it's a separate `[[bin]]` target but only dev-facing).

use tracing::{error, info, warn};

fn main() {
    beava_server::logging::init("info").expect("init logging");
    info!(target: "beava.probe", version = env!("CARGO_PKG_VERSION"), "probe started");
    warn!(target: "beava.probe", code = 42i64, "probe warn event");
    error!(target: "beava.probe", "probe error event");
}
