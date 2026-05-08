//! Centralized default values for devex-first zero-config registration.
//!
//! These values materialize at runtime (push handlers, query handlers) when
//! the descriptor field is None. The descriptor preserves the user's literal
//! choice — None means "use the default below"; Some(N) means "user chose N".

/// Default retention horizon for raw events. If `keep_events_for_ms` is None,
/// events are kept for this duration before eviction.
pub const DEFAULT_KEEP_EVENTS_FOR_MS: u64 = 604_800_000; // 7 days

/// Default dedupe window when `dedupe_key` is set but `dedupe_window_ms` is None.
pub const DEFAULT_DEDUPE_WINDOW_MS: u64 = 86_400_000; // 24 hours

/// Default TCP listen host for the binary-framed wire.
pub const DEFAULT_TCP_HOST: &str = "127.0.0.1";

/// Default TCP port for the binary-framed wire. Locked v0 surface
/// (STATE.md surface-lock 2026-05-06): HTTP on 8080, TCP on 8081.
pub const DEFAULT_TCP_PORT: u16 = 8081;

/// Default max frame size in bytes (4 MiB). Oversize frames produce a
/// `frame_too_large` error response, then the connection closes.
/// Recommended operator ceiling: 16 MiB; the u32 upper bound is 4 GiB and
/// would OOM a typical box if configured that high.
pub const DEFAULT_TCP_MAX_FRAME_BYTES: u32 = 4 * 1024 * 1024; // 4 MiB
