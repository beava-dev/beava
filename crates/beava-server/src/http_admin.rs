//! Admin HTTP plane — tokio/axum app on a dedicated port.
//!
//! Handles `/health`, `/ready`, `/metrics`, `/registry`. Read-only access to
//! shared state via `Arc<RwLock<RegistrySnapshot>>`; the event-plane state is
//! updated only by the mio data plane (mio-only invariant). This module is
//! the only legitimate home for `axum::*` symbols in beava-server.

use axum::{
    extract::State,
    http::{header::HeaderName, HeaderValue, StatusCode},
    middleware,
    middleware::Next,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use beava_core::agg_state::{
    BucketReclaimCounter, ColdEntityEvictionCounter, EntityCountResidentSnapshot, EntropyStateWrap,
};
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;

/// Axum middleware that stamps every admin response with
/// `X-Runtime: tokio`. Data-plane routes return `X-Runtime: mio` so callers
/// can tell the two runtimes apart at the HTTP layer.
async fn stamp_tokio_header(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        HeaderName::from_static("x-runtime"),
        HeaderValue::from_static("tokio"),
    );
    response
}

/// A point-in-time snapshot of registry metadata for admin read access.
/// Updated on every successful register call from the event-plane.
#[derive(Clone, Debug, Default)]
pub struct RegistrySnapshot {
    /// Number of registered aggregation nodes.
    pub node_count: usize,
    /// Registry version (monotonic counter).
    pub version: u64,
}

pub type SharedRegistrySnapshot = Arc<RwLock<RegistrySnapshot>>;

#[derive(Clone)]
struct AdminState {
    snapshot: SharedRegistrySnapshot,
}

pub fn admin_router(snapshot: SharedRegistrySnapshot) -> Router {
    let state = AdminState { snapshot };
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
        .route("/registry", get(registry_handler))
        .with_state(state)
        .layer(middleware::from_fn(stamp_tokio_header))
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

async fn ready_handler() -> impl IntoResponse {
    // Event-plane recovery completes before `ServerV18::bind` returns, so
    // by the time the admin listener is up the server is ready to serve.
    (StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
}

async fn metrics_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let snap = state
        .snapshot
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    // Prometheus exposition format (text/plain; version=0.0.4). All v0
    // counters are unlabeled — per-source labels are deferred work.
    let entropy_capped = EntropyStateWrap::categories_capped_count();
    let cold_evictions = ColdEntityEvictionCounter::count();
    let bucket_reclaims = BucketReclaimCounter::count();
    let entity_count = EntityCountResidentSnapshot::load();
    // Static v0 estimate from PROJECT.md "Memory" budget (~7 KB per entity
    // for a rich 30-feature pack). A periodic resampler is post-v0 work.
    const BYTES_PER_ENTITY_P99_V0_PLACEHOLDER: u64 = 7000;
    // The lifetime-op aggregate counter currently aliases entropy
    // `categories_capped`; top-k displacement and histogram bucket drops
    // join when those operator internals expose hooks.
    let op_cap_hits = entropy_capped;
    let snapshot_metrics = crate::snapshot_metrics::snapshot();
    let snapshot_duration_seconds = snapshot_metrics.last_duration_us as f64 / 1_000_000.0;
    let snapshot_fsync_seconds = snapshot_metrics.last_fsync_us as f64 / 1_000_000.0;

    let body = format!(
        "# HELP beava_registry_version Registry version (monotonic).\n\
         # TYPE beava_registry_version gauge\n\
         beava_registry_version {registry_version}\n\
         # HELP beava_node_count Number of registered aggregation nodes.\n\
         # TYPE beava_node_count gauge\n\
         beava_node_count {node_count}\n\
         # HELP beava_runtime_kind Data-plane runtime (1=active). Labels: runtime.\n\
         # TYPE beava_runtime_kind gauge\n\
         beava_runtime_kind{{runtime=\"mio\"}} 1\n\
         # HELP beava_entropy_categories_capped_total Total new-category insertions dropped due to max_categories cap.\n\
         # TYPE beava_entropy_categories_capped_total counter\n\
         beava_entropy_categories_capped_total {entropy_capped}\n\
         # HELP beava_cold_entity_evictions_total Total cold-TTL entity evictions since process start. Increments when an entity arrives after `now_ms - last_seen_ms > cold_after_ms`.\n\
         # TYPE beava_cold_entity_evictions_total counter\n\
         beava_cold_entity_evictions_total {cold_evictions}\n\
         # HELP beava_lifetime_op_cap_hit_total Total cap-hit events across lifetime aggregation operators. Currently aggregates entropy `categories_capped`; top_k displacements + histogram bucket drops join when their internals expose hooks.\n\
         # TYPE beava_lifetime_op_cap_hit_total counter\n\
         beava_lifetime_op_cap_hit_total {op_cap_hits}\n\
         # HELP beava_entity_count_resident Current resident entity count across all sources / aggregations. Snapshot refreshed by the apply path post-update; admin reads via a process-static atomic load (zero-lock).\n\
         # TYPE beava_entity_count_resident gauge\n\
         beava_entity_count_resident {entity_count}\n\
         # HELP beava_bucket_reclaim_total Total trailing-bucket evictions on windowed operators. Increments on each WindowedOp::evict_oldest_bucket call (64-bucket cap).\n\
         # TYPE beava_bucket_reclaim_total counter\n\
         beava_bucket_reclaim_total {bucket_reclaims}\n\
         # HELP beava_bytes_per_entity_p99 Static estimate of per-entity memory footprint (~7 KB for a rich 30-feature pack).\n\
         # TYPE beava_bytes_per_entity_p99 gauge\n\
         beava_bytes_per_entity_p99 {bytes_per_entity_p99}\n\
         # HELP beava_snapshot_last_duration_seconds Wall-clock duration of the last successful snapshot.\n\
         # TYPE beava_snapshot_last_duration_seconds gauge\n\
         beava_snapshot_last_duration_seconds {snapshot_duration_seconds:.6}\n\
         # HELP beava_snapshot_last_bytes Bytes written by the last successful snapshot, including snapshot header and body.\n\
         # TYPE beava_snapshot_last_bytes gauge\n\
         beava_snapshot_last_bytes {snapshot_bytes}\n\
         # HELP beava_snapshot_last_fsync_seconds File plus parent-directory fsync time for the last successful snapshot.\n\
         # TYPE beava_snapshot_last_fsync_seconds gauge\n\
         beava_snapshot_last_fsync_seconds {snapshot_fsync_seconds:.6}\n",
        registry_version = snap.version,
        node_count = snap.node_count,
        entropy_capped = entropy_capped,
        cold_evictions = cold_evictions,
        op_cap_hits = op_cap_hits,
        entity_count = entity_count,
        bucket_reclaims = bucket_reclaims,
        bytes_per_entity_p99 = BYTES_PER_ENTITY_P99_V0_PLACEHOLDER,
        snapshot_duration_seconds = snapshot_duration_seconds,
        snapshot_bytes = snapshot_metrics.last_bytes,
        snapshot_fsync_seconds = snapshot_fsync_seconds,
    );

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

async fn registry_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let snap = state
        .snapshot
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "version": snap.version,
            "node_count": snap.node_count,
        })),
    )
}

/// A bound admin HTTP server handle. Holds the local address and a shutdown
/// channel. Dropped when `ServerV18` shuts down.
pub struct BoundAdminServer {
    pub local_addr: std::net::SocketAddr,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    join: tokio::task::JoinHandle<()>,
}

impl BoundAdminServer {
    /// Bind and start the admin axum server on `addr`. Returns immediately after
    /// the listener is bound; the serve loop runs on a tokio task.
    pub async fn bind(
        addr: std::net::SocketAddr,
        snapshot: SharedRegistrySnapshot,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let app = admin_router(snapshot);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let shutdown_signal = async move {
            let _ = rx.await;
        };
        let join = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal)
                .await;
        });
        Ok(Self {
            local_addr,
            shutdown_tx: tx,
            join,
        })
    }

    /// Gracefully stop the admin server.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        let _ = self.join.await;
    }
}
