//! AggOp enum dispatch + AggOpDescriptor + output_type_for.
//!
//! This module provides:
//! - `AggKind`: 8-variant enum identifying the aggregation operation kind.
//! - `AggOpDescriptor`: register-time descriptor (kind, field, window_ms).
//! - `AggOp`: live state enum dispatching update/query via match arms (no
//!   Box<dyn>). Per D-01.
//! - `output_type_for`: register-time type inference mapping op kind →
//!   FieldType. Used by Plan 05-03 schema propagator.
//!
//! # Requirements traceability
//! - AGG-CORE-01..08: covered per-op in agg_state.rs
//! - AGG-CORE-09: Windowed<Op> via WindowedOp in agg_windowed.rs
//!
//! D-01: enum + match arm dispatch; no Box<dyn AggOp>.
//! D-06: no wall-clock reads in apply paths (forbidden: SystemTime now, rand).

use crate::agg_buffer::{
    DowHourHistogramState, EventTypeMixState, HistogramState, HourOfDayHistogramState,
    MostRecentNState, ReservoirSampleState, SeasonalDeviationState,
};
use crate::agg_geo::{DistanceFromHomeState, GeoDistanceState, GeoSpreadState, GeoVelocityState};
use crate::agg_state::{
    AvgState, BloomMemberStateWrap, CountDistinctStateWrap, CountState, EntropyStateWrap,
    FirstNState, FirstSeenInWindowState, FirstState, LagState, LastNState, LastState, MaxState,
    MinState, NegativeStreakState, PercentileStateWrap, RatioState, SeenState, StreakState,
    SumState, TimeSinceLastNState, TopKStateWrap, VarianceState,
};
use crate::agg_state_decay::{
    DecayedCountState, DecayedSumState, EwVarState, EwZScoreState, EwmaState, TwaState,
};
use crate::agg_state_velocity::{
    BurstCountState, DeltaFromPrevState, InterArrivalStatsState, OutlierCountState,
    RateOfChangeState, TrendResidualState, TrendState, ValueChangeCountState, ZScoreState,
};
use crate::row::{Row, Value};
use crate::schema::FieldType;
use crate::schema_propagate::Schema;
use serde::{Deserialize, Serialize};

// Forward declaration — implementation in agg_windowed.rs (Task 2).
// We import it here so AggOp::Windowed can hold it.
use crate::agg_windowed::WindowedOp;

// ─── AggKind ─────────────────────────────────────────────────────────────────

/// Identifies the aggregation operation kind. Copy + Clone for use in
/// descriptors and WindowedOp inner_kind (no heap allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggKind {
    // ── Phase 5: core ────────────────────────────────────────────────────
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Variance,
    StdDev,
    Ratio,
    // ── Phase 8: point/ordinal ────────────────────────────────────────────────
    First,
    Last,
    FirstN,
    LastN,
    Lag,
    // ── Phase 8: recency markers ──────────────────────────────────────────────
    FirstSeen,
    LastSeen,
    Age,
    HasSeen,
    TimeSince,
    TimeSinceLastN,
    // ── Phase 8: streaks ──────────────────────────────────────────────────────
    Streak,
    MaxStreak,
    NegativeStreak,
    // ── Phase 8: windowed recency ─────────────────────────────────────────────
    FirstSeenInWindow,
    // ── Phase 9: decay (AGG-DECAY-01..06) ─────────────────────────────────
    Ewma,
    EwVar,
    EwZScore,
    DecayedSum,
    DecayedCount,
    Twa,
    // ── Phase 9: velocity (AGG-VEL-01..08) ────────────────────────────────
    RateOfChange,
    InterArrivalStats,
    BurstCount,
    DeltaFromPrev,
    Trend,
    TrendResidual,
    OutlierCount,
    ValueChangeCount,
    // ── Phase 9: entity z-score (AGG-Z-01) ────────────────────────────────
    ZScore,
    // ── Phase 10: sketches ───────────────────────────────────────────────
    /// AGG-SKETCH-01 (Plan 10-05)
    CountDistinct,
    /// AGG-SKETCH-02 (Plan 10-05)
    Percentile,
    /// AGG-SKETCH-03 (Plan 10-05)
    TopK,
    /// AGG-SKETCH-04 (Plan 10-05) — windowless-only
    BloomMember,
    /// AGG-SKETCH-05 (Plan 10-05)
    Entropy,
    // ── Phase 11: bounded-buffer + geo ───────────────────────────────────────
    Histogram,
    HourOfDayHistogram,
    DowHourHistogram,
    SeasonalDeviation,
    EventTypeMix,
    MostRecentN,
    ReservoirSample,
    GeoVelocity,
    GeoDistance,
    GeoSpread,
    DistanceFromHome,
}

/// Optional sketch construction params attached to AggOpDescriptor.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SketchParams {
    pub percentile_q: Option<f64>,
    pub top_k_k: Option<usize>,
    pub bloom_capacity: Option<usize>,
    pub bloom_fpr: Option<f64>,
}

/// Extended params for buffer + geo operators.
/// Default = empty (None-valued); core/sketch ops never consult this struct.
///
/// `lat_idx` + `lon_idx` are register-time-resolved indices into the apply-loop's
/// `ExtractedFields` array. Sentinel `FIELD_IDX_NONE` (u8::MAX) means
/// "not resolved yet" or "geo op not present".
#[derive(Debug, Clone)]
pub struct AggExtParams {
    pub buckets: Option<Vec<f64>>,
    pub n: Option<usize>,
    pub k: Option<usize>,
    pub precision: Option<u32>,
    pub lat_field: Option<String>,
    pub lon_field: Option<String>,
    pub samples: Option<usize>,
    pub categories: Option<Vec<String>>,
    pub max_categories: Option<usize>,
    /// Resolved index of the latitude field in `ExtractedFields`.
    /// `FIELD_IDX_NONE` = not a geo op / not yet resolved.
    pub lat_idx: u8,
    /// Resolved index of the longitude field in `ExtractedFields`.
    /// `FIELD_IDX_NONE` = not a geo op / not yet resolved.
    pub lon_idx: u8,
}

impl Default for AggExtParams {
    fn default() -> Self {
        Self {
            buckets: None,
            n: None,
            k: None,
            precision: None,
            lat_field: None,
            lon_field: None,
            samples: None,
            categories: None,
            max_categories: None,
            lat_idx: FIELD_IDX_NONE,
            lon_idx: FIELD_IDX_NONE,
        }
    }
}

// reason: explicit Default impl makes the chosen default (Count) discoverable
// at the type definition; #[derive(Default)] would silently pick the first
// variant and shift on enum reordering.
#[allow(clippy::derivable_impls)]
impl Default for AggKind {
    fn default() -> Self {
        AggKind::Count
    }
}

impl AggKind {
    pub fn supports_windowed_wrap(self) -> bool {
        matches!(
            self,
            AggKind::Count
                | AggKind::Sum
                | AggKind::Avg
                | AggKind::Min
                | AggKind::Max
                | AggKind::Variance
                | AggKind::StdDev
                | AggKind::Ratio
                | AggKind::CountDistinct
                | AggKind::Percentile
                | AggKind::TopK
                | AggKind::Entropy
        )
    }

    pub fn requires_half_life(self) -> bool {
        matches!(
            self,
            AggKind::Ewma
                | AggKind::EwVar
                | AggKind::EwZScore
                | AggKind::DecayedSum
                | AggKind::DecayedCount
        )
    }
}

// ─── Field index constants ────────────────────────────────────────────────────

/// Sentinel value for `AggOpDescriptor.field_idx` when the op has no field
/// (Count, Ratio, HourOfDayHistogram, DowHourHistogram) or when resolution
/// has not yet been performed.
///
/// Using `u8::MAX` (255) as sentinel keeps the value fitting in a byte while
/// leaving 0..=254 for actual indices. Since Beava limits events to ≤8 inline
/// fields (Row SmallVec capacity), indices 0..7 cover the steady-state case.
pub const FIELD_IDX_NONE: u8 = u8::MAX;

// ─── ExtractedFields ─────────────────────────────────────────────────────────

/// Pre-extracted field values for one event, indexed by
/// `AggOpDescriptor.field_idx`. Length matches `AggregationDescriptor.field_names`.
/// Built ONCE per event at apply-loop entry; consumed by every per-feature update call.
///
/// SmallVec inline capacity = 16: covers fraud-team's per-source union (~12 fields
/// max for the TxnByUser cluster) without spilling. Earlier cap was 8 — flamegraph
/// showed `RawVec::with_capacity_in` + `RawVecInner::reserve` together at ~4.0%
/// inclusive on the apply hot path, 99% from this SmallVec spilling on every Txn
/// event (~530 ns/event of allocator traffic). Cap widening eliminates the heap
/// spill; the apply path stays inline-only for fraud-team's per-source field union.
pub type ExtractedFields<'a> = smallvec::SmallVec<[Option<&'a crate::row::Value>; 16]>;

// ─── AggOpDescriptor ─────────────────────────────────────────────────────────

/// Register-time descriptor for one aggregation feature.
///
/// `where_expr` gates the apply-path update via `agg_where::evaluate_where_predicate`.
/// Added in Plan 05-02 (predicate threading, SDK-AGG-04).
///
/// `ext` carries Phase 11-family optional params (buckets, n, k, precision,
/// lat_field, lon_field, samples, categories). Default = empty / no extended
/// config so existing core ops stay source-compatible.
///
/// `field_idx` (Plan 19.2-01 D-01): pre-resolved index into the apply-loop's
/// `ExtractedFields` array. Populated by `Registry::resolve_field_indices` at
/// apply_registration time. `FIELD_IDX_NONE` (u8::MAX) = no field needed.
#[derive(Debug, Clone)]
pub struct AggOpDescriptor {
    pub kind: AggKind,
    /// Field name for Sum/Avg/Min/Max/Variance/StdDev. None for Count/Ratio.
    pub field: Option<String>,
    /// Tumbling window duration in milliseconds. None = lifetime (windowless).
    pub window_ms: Option<u64>,
    /// Optional where-predicate (Plan 05-02). Evaluated via
    /// `agg_where::evaluate_where_predicate` at apply time.
    /// None = unconditional update (backwards-compatible with Plan 05-01).
    pub where_expr: Option<std::sync::Arc<crate::expr::Expr>>,
    /// Bounded-buffer size parameter for `first_n`, `last_n`, `lag`,
    /// `time_since_last_n`. `None` for ops that don't take an `n` param.
    pub n: Option<u32>,
    /// Required for decay ops (`AggKind::requires_half_life()`); ignored otherwise.
    pub half_life_ms: Option<u64>,
    /// Required for `BurstCount`; ignored otherwise.
    pub sub_window_ms: Option<u64>,
    /// Defaults to 3.0 for `OutlierCount`; ignored otherwise.
    pub sigma: Option<f64>,
    /// Per-op sketch construction params (k, q, fpr, capacity). None for non-sketch ops.
    pub sketch_params: Option<SketchParams>,
    /// Extended params for buffer + geo operators (None-valued for other ops).
    pub ext: AggExtParams,
    /// Pre-resolved index into the apply-loop's `ExtractedFields` array.
    /// Populated by `Registry::resolve_field_indices` at `apply_registration` time.
    /// Default = `FIELD_IDX_NONE`; client-supplied JSON omits this field
    /// (resolved server-side only). Not serialized: `AggOpDescriptor` is not a
    /// serde type; `field_idx` is computed server-side at registration and never
    /// transported over the wire.
    pub field_idx: u8,
    /// Per-agg-feature mapping from the agg's local field positions (i.e.
    /// `field_idx`, also `agg.field_names` index) to the per-source-event
    /// `apply_field_names` union indices.
    ///
    /// Populated by `Registry::resolve_field_indices_for_agg_mut*` at register
    /// time (alongside `field_idx`). Empty for features with no declared field
    /// (e.g. `AggKind::Count`).
    ///
    /// Indexed by the agg's local field position: for a feature that declares
    /// `field: Some("amount")`, when "amount" is at position `i` in
    /// `agg.field_names` and at position `j` in
    /// `EventDescriptor.apply_field_names`, then
    /// `field_idx_into_event_extracted[i] == j`.
    ///
    /// Used by the apply-loop hoist (Task 4.3) to remap `field_idx`-style
    /// lookups against the per-event union slice without per-descriptor
    /// rebuild scaffolding.
    ///
    /// `AggOpDescriptor` is not a serde type; this field is computed
    /// server-side at registration and never transported over the wire
    /// (snapshot replay re-runs the resolver against the same alphabetical
    /// `apply_field_names` ordering).
    pub field_idx_into_event_extracted: Vec<u8>,
}

impl Default for AggOpDescriptor {
    /// Default = lifetime Count with no field/window/where (parses as `bv.count()`).
    fn default() -> Self {
        AggOpDescriptor {
            kind: AggKind::Count,
            field: None,
            window_ms: None,
            where_expr: None,
            n: None,
            half_life_ms: None,
            sub_window_ms: None,
            sigma: None,
            sketch_params: None,
            ext: AggExtParams::default(),
            field_idx: FIELD_IDX_NONE,
            field_idx_into_event_extracted: Vec::new(),
        }
    }
}

// ─── AggTypeError ────────────────────────────────────────────────────────────

/// Error type for `output_type_for` register-time type inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggTypeError {
    /// A field-based op (Min/Max) requires `desc.field` to be set.
    FieldRequired { kind: AggKind },
    /// The specified field is not present in the upstream schema.
    FieldMissing { field: String },
}

impl std::fmt::Display for AggTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AggTypeError::FieldRequired { kind } => {
                write!(f, "aggregation {:?} requires a field to be specified", kind)
            }
            AggTypeError::FieldMissing { field } => {
                write!(f, "field '{}' not found in upstream schema", field)
            }
        }
    }
}

// ─── AggOp ───────────────────────────────────────────────────────────────────

/// Wrapper bundling a decay op state with its constant `half_life_ms` parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EwmaOp {
    pub state: EwmaState,
    pub half_life_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EwVarOp {
    pub state: EwVarState,
    pub half_life_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EwZScoreOp {
    pub state: EwZScoreState,
    pub half_life_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayedSumOp {
    pub state: DecayedSumState,
    pub half_life_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayedCountOp {
    pub state: DecayedCountState,
    pub half_life_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurstCountOp {
    pub state: BurstCountState,
    pub sub_window_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlierCountOp {
    pub state: OutlierCountState,
    pub sigma: f64,
}

/// Live per-(feature, entity) aggregation state. Enum dispatch; no Box<dyn>.
///
/// Phase 12.9: the 7 fat-payload variants — `SeasonalDeviation`,
/// `HourOfDayHistogram`, `EventTypeMix`, `GeoVelocity`, `GeoSpread`,
/// `GeoDistance`, `DistanceFromHome` — are `Box`-wrapped (mirroring sketch
/// variants and `WindowedOp`). The earlier unboxed layout pushed fraud-team
/// to ~22 KB/entity (3× the CLAUDE.md 7 KB budget); boxing dropped
/// `size_of::<AggOp>()` from 600 B to ~72 B (8× shrink) with per-event update
/// within ±5% on the small/tcp regression-gate. Match arms auto-deref through
/// `Box::DerefMut`, so dispatch sites stay unchanged.
///
/// CI tripwire: `crates/beava-core/tests/per_entity_size_dump.rs::aggop_size_within_cap`
/// asserts `size_of::<AggOp>() <= 80`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggOp {
    // ── Core scalar ──────────────────────────────────────────────────────────
    Count(CountState),
    Sum(SumState),
    Avg(AvgState),
    Min(MinState),
    Max(MaxState),
    Variance(VarianceState),
    StdDev(VarianceState),
    Ratio(RatioState),
    // ── Sketches ───────────────────────────────────────────────
    /// AGG-SKETCH-01
    CountDistinct(Box<CountDistinctStateWrap>),
    /// AGG-SKETCH-02
    Percentile(Box<PercentileStateWrap>),
    /// AGG-SKETCH-03
    TopK(Box<TopKStateWrap>),
    /// AGG-SKETCH-04
    BloomMember(Box<BloomMemberStateWrap>),
    /// AGG-SKETCH-05
    Entropy(Box<EntropyStateWrap>),
    /// AGG-CORE-09: any op wrapped in 64-bucket event-time tumbling
    Windowed(Box<WindowedOp>),
    // ── Point / ordinal ────────────────────────────────────────────────
    First(FirstState),
    Last(LastState),
    FirstN(FirstNState),
    LastN(LastNState),
    Lag(LagState),
    // ── Recency markers ──────────────────────────────────────────────
    FirstSeen(SeenState),
    LastSeen(SeenState),
    Age(SeenState),
    HasSeen(SeenState),
    TimeSince(SeenState),
    TimeSinceLastN(TimeSinceLastNState),
    // ── Streak ───────────────────────────────────────────────────────
    Streak(StreakState),
    MaxStreak(StreakState),
    NegativeStreak(NegativeStreakState),
    // ── Windowed recency (lifetime-state) ────────────────────────────
    FirstSeenInWindow(FirstSeenInWindowState),
    // ── Decay ────────────────────────────────────────────────────
    Ewma(EwmaOp),
    EwVar(EwVarOp),
    EwZScore(EwZScoreOp),
    DecayedSum(DecayedSumOp),
    DecayedCount(DecayedCountOp),
    Twa(TwaState),
    // ── Velocity ─────────────────────────────────────────────────
    RateOfChange(RateOfChangeState),
    InterArrivalStats(InterArrivalStatsState),
    BurstCount(BurstCountOp),
    DeltaFromPrev(DeltaFromPrevState),
    Trend(TrendState),
    TrendResidual(TrendResidualState),
    OutlierCount(OutlierCountOp),
    ValueChangeCount(ValueChangeCountState),
    // ── Entity z-score ───────────────────────────────────────────
    ZScore(ZScoreState),
    // ── Buffer + geo (always windowless in v0) ───────────────────────
    Histogram(HistogramState),
    HourOfDayHistogram(Box<HourOfDayHistogramState>),
    DowHourHistogram(DowHourHistogramState),
    SeasonalDeviation(Box<SeasonalDeviationState>),
    EventTypeMix(Box<EventTypeMixState>),
    MostRecentN(MostRecentNState),
    ReservoirSample(ReservoirSampleState),
    GeoVelocity(Box<GeoVelocityState>),
    GeoDistance(Box<GeoDistanceState>),
    GeoSpread(Box<GeoSpreadState>),
    DistanceFromHome(Box<DistanceFromHomeState>),
}

impl AggOp {
    /// Construct the live state for a descriptor.
    ///
    /// If `desc.window_ms.is_some()`, wraps the inner op in `WindowedOp`.
    /// Otherwise returns the lifetime (windowless) variant.
    ///
    /// Buffer/geo operators are always windowless in v0; the register-time
    /// compiler rejects `window=...` for those op names.
    pub fn new(desc: &AggOpDescriptor) -> Self {
        // Core + sketch ops support Windowed wrap. Point/recency/decay/velocity/
        // buffer/geo ops are lifetime-only. `FirstSeenInWindow` carries
        // window_ms as a lifetime parameter (NOT a tumbling-bucket window).
        if let Some(window_ms) = desc.window_ms {
            if matches!(desc.kind, AggKind::FirstSeenInWindow) {
                return AggOp::FirstSeenInWindow(FirstSeenInWindowState::new(window_ms));
            }
            if desc.kind.supports_windowed_wrap() {
                return AggOp::Windowed(Box::new(WindowedOp::new_with_params(
                    desc.kind,
                    window_ms,
                    desc.sketch_params.clone().unwrap_or_default(),
                )));
            }
            // Lifetime-only ops with a window= silently fall through to lifetime
            // construction (compile-time validation should have rejected this).
        }
        // Inline rather than calling `new_lifetime` because lifetime ops need
        // extra descriptor fields (n, half_life_ms, sub_window_ms, sigma) that
        // `new_lifetime` (used by sketch bucket init) doesn't carry.
        AggOp::new_lifetime_full(desc)
    }

    /// Full-descriptor lifetime construction. Used by `AggOp::new` for the
    /// windowless path. Honors `n`, `half_life_ms`, `sub_window_ms`, `sigma`,
    /// and sketch params.
    pub fn new_lifetime_full(desc: &AggOpDescriptor) -> Self {
        let sp_default = SketchParams::default();
        let sp = desc.sketch_params.as_ref().unwrap_or(&sp_default);
        match desc.kind {
            AggKind::Count => AggOp::Count(CountState::default()),
            AggKind::Sum => AggOp::Sum(SumState::default()),
            AggKind::Avg => AggOp::Avg(AvgState::default()),
            AggKind::Min => AggOp::Min(MinState::default()),
            AggKind::Max => AggOp::Max(MaxState::default()),
            AggKind::Variance => AggOp::Variance(VarianceState::default()),
            AggKind::StdDev => AggOp::StdDev(VarianceState::default()),
            AggKind::Ratio => AggOp::Ratio(RatioState::default()),
            // Point/ordinal
            AggKind::First => AggOp::First(FirstState::default()),
            AggKind::Last => AggOp::Last(LastState::default()),
            AggKind::FirstN => AggOp::FirstN(FirstNState::new(desc.n.unwrap_or(1))),
            AggKind::LastN => AggOp::LastN(LastNState::new(desc.n.unwrap_or(1))),
            AggKind::Lag => AggOp::Lag(LagState::new(desc.n.unwrap_or(1))),
            // Recency markers
            AggKind::FirstSeen => AggOp::FirstSeen(SeenState::default()),
            AggKind::LastSeen => AggOp::LastSeen(SeenState::default()),
            AggKind::Age => AggOp::Age(SeenState::default()),
            AggKind::HasSeen => AggOp::HasSeen(SeenState::default()),
            AggKind::TimeSince => AggOp::TimeSince(SeenState::default()),
            AggKind::TimeSinceLastN => {
                AggOp::TimeSinceLastN(TimeSinceLastNState::new(desc.n.unwrap_or(1)))
            }
            // Streak
            AggKind::Streak => AggOp::Streak(StreakState::default()),
            AggKind::MaxStreak => AggOp::MaxStreak(StreakState::default()),
            AggKind::NegativeStreak => AggOp::NegativeStreak(NegativeStreakState::default()),
            // Windowed recency
            AggKind::FirstSeenInWindow => {
                AggOp::FirstSeenInWindow(FirstSeenInWindowState::new(desc.window_ms.unwrap_or(0)))
            }
            // Decay
            AggKind::Ewma => AggOp::Ewma(EwmaOp {
                state: EwmaState::default(),
                half_life_ms: desc.half_life_ms.unwrap_or(1),
            }),
            AggKind::EwVar => AggOp::EwVar(EwVarOp {
                state: EwVarState::default(),
                half_life_ms: desc.half_life_ms.unwrap_or(1),
            }),
            AggKind::EwZScore => AggOp::EwZScore(EwZScoreOp {
                state: EwZScoreState::default(),
                half_life_ms: desc.half_life_ms.unwrap_or(1),
            }),
            AggKind::DecayedSum => AggOp::DecayedSum(DecayedSumOp {
                state: DecayedSumState::default(),
                half_life_ms: desc.half_life_ms.unwrap_or(1),
            }),
            AggKind::DecayedCount => AggOp::DecayedCount(DecayedCountOp {
                state: DecayedCountState::default(),
                half_life_ms: desc.half_life_ms.unwrap_or(1),
            }),
            AggKind::Twa => AggOp::Twa(TwaState::default()),
            // Velocity
            AggKind::RateOfChange => AggOp::RateOfChange(RateOfChangeState::default()),
            AggKind::InterArrivalStats => {
                AggOp::InterArrivalStats(InterArrivalStatsState::default())
            }
            AggKind::BurstCount => AggOp::BurstCount(BurstCountOp {
                state: BurstCountState::default(),
                sub_window_ms: desc.sub_window_ms.unwrap_or(1),
            }),
            AggKind::DeltaFromPrev => AggOp::DeltaFromPrev(DeltaFromPrevState::default()),
            AggKind::Trend => AggOp::Trend(TrendState::default()),
            AggKind::TrendResidual => AggOp::TrendResidual(TrendResidualState::default()),
            AggKind::OutlierCount => AggOp::OutlierCount(OutlierCountOp {
                state: OutlierCountState::default(),
                sigma: desc.sigma.unwrap_or(3.0),
            }),
            AggKind::ValueChangeCount => AggOp::ValueChangeCount(ValueChangeCountState::default()),
            AggKind::ZScore => AggOp::ZScore(ZScoreState::default()),
            // Sketches
            AggKind::CountDistinct => AggOp::CountDistinct(Box::default()),
            AggKind::Percentile => {
                let mut s = PercentileStateWrap::default();
                if let Some(q) = sp.percentile_q {
                    s.q = q;
                }
                AggOp::Percentile(Box::new(s))
            }
            AggKind::TopK => {
                let k = sp.top_k_k.unwrap_or(10).max(1);
                AggOp::TopK(Box::new(TopKStateWrap {
                    inner: crate::sketches::top_k::TopKState::new(k, 1024, 2048, 4),
                }))
            }
            AggKind::BloomMember => {
                let cap = sp.bloom_capacity.unwrap_or(1024);
                let fpr = sp.bloom_fpr.unwrap_or(0.01);
                AggOp::BloomMember(Box::new(BloomMemberStateWrap::with_params(cap, fpr)))
            }
            AggKind::Entropy => AggOp::Entropy(Box::default()),
            // Buffer + geo
            AggKind::Histogram => AggOp::Histogram(HistogramState::new(
                desc.ext.buckets.clone().unwrap_or_default(),
            )),
            AggKind::HourOfDayHistogram => AggOp::HourOfDayHistogram(Box::default()),
            AggKind::DowHourHistogram => AggOp::DowHourHistogram(DowHourHistogramState::default()),
            AggKind::SeasonalDeviation => AggOp::SeasonalDeviation(Box::default()),
            AggKind::EventTypeMix => AggOp::EventTypeMix(Box::new(EventTypeMixState::new(
                desc.ext.max_categories.unwrap_or(256),
                desc.ext.categories.clone(),
            ))),
            AggKind::MostRecentN => {
                AggOp::MostRecentN(MostRecentNState::new(desc.ext.n.unwrap_or(10)))
            }
            AggKind::ReservoirSample => AggOp::ReservoirSample(ReservoirSampleState::new(
                desc.ext.samples.or(desc.ext.k).unwrap_or(10),
            )),
            AggKind::GeoVelocity => AggOp::GeoVelocity(Box::new(GeoVelocityState::with_fields(
                desc.ext.lat_field.clone().unwrap_or_else(|| "lat".into()),
                desc.ext.lon_field.clone().unwrap_or_else(|| "lon".into()),
            ))),
            AggKind::GeoDistance => AggOp::GeoDistance(Box::new(GeoDistanceState::with_fields(
                desc.ext.lat_field.clone().unwrap_or_else(|| "lat".into()),
                desc.ext.lon_field.clone().unwrap_or_else(|| "lon".into()),
            ))),
            AggKind::GeoSpread => AggOp::GeoSpread(Box::new(GeoSpreadState::with_fields(
                desc.ext.lat_field.clone().unwrap_or_else(|| "lat".into()),
                desc.ext.lon_field.clone().unwrap_or_else(|| "lon".into()),
            ))),
            AggKind::DistanceFromHome => {
                AggOp::DistanceFromHome(Box::new(DistanceFromHomeState::with_fields(
                    desc.ext.lat_field.clone().unwrap_or_else(|| "lat".into()),
                    desc.ext.lon_field.clone().unwrap_or_else(|| "lon".into()),
                    desc.ext.samples.unwrap_or(100),
                )))
            }
        }
    }

    /// Construct a fresh lifetime (windowless) AggOp for `kind` with optional sketch params.
    /// Used by both `AggOp::new` and `WindowedOp` bucket initialisation.
    pub fn new_lifetime(kind: AggKind, sketch_params: Option<&SketchParams>) -> Self {
        let sp_default = SketchParams::default();
        let sp = sketch_params.unwrap_or(&sp_default);
        match kind {
            AggKind::Count => AggOp::Count(CountState::default()),
            AggKind::Sum => AggOp::Sum(SumState::default()),
            AggKind::Avg => AggOp::Avg(AvgState::default()),
            AggKind::Min => AggOp::Min(MinState::default()),
            AggKind::Max => AggOp::Max(MaxState::default()),
            AggKind::Variance => AggOp::Variance(VarianceState::default()),
            AggKind::StdDev => AggOp::StdDev(VarianceState::default()),
            AggKind::Ratio => AggOp::Ratio(RatioState::default()),
            // Sketches (only path that carries `sketch_params`).
            AggKind::CountDistinct => AggOp::CountDistinct(Box::default()),
            AggKind::Percentile => {
                let mut s = PercentileStateWrap::default();
                if let Some(q) = sp.percentile_q {
                    s.q = q;
                }
                AggOp::Percentile(Box::new(s))
            }
            AggKind::TopK => {
                let k = sp.top_k_k.unwrap_or(10).max(1);
                AggOp::TopK(Box::new(TopKStateWrap {
                    inner: crate::sketches::top_k::TopKState::new(k, 1024, 2048, 4),
                }))
            }
            AggKind::BloomMember => {
                let cap = sp.bloom_capacity.unwrap_or(1024);
                let fpr = sp.bloom_fpr.unwrap_or(0.01);
                AggOp::BloomMember(Box::new(BloomMemberStateWrap::with_params(cap, fpr)))
            }
            AggKind::Entropy => AggOp::Entropy(Box::default()),
            // Lifetime-only ops (point / recency / streak / decay / velocity / buffer / geo)
            // never reach this path — they aren't wrapped in WindowedOp (the only caller of
            // `new_lifetime`). For safety, delegate to `new_lifetime_full` with a default
            // descriptor so this stays correct if a future caller appears.
            other => AggOp::new_lifetime_full(&AggOpDescriptor {
                kind: other,
                ..Default::default()
            }),
        }
    }

    /// Returns true iff the op is a core scalar op that supports the 64-bucket
    /// Windowed wrap. Used by serialization/debugging helpers; keep crate-public.
    // reason: kept crate-public for serialization / debugging helpers per the
    // doc-comment above; current call sites are cfg-gated.
    #[allow(dead_code)]
    pub(crate) fn is_core(&self) -> bool {
        matches!(
            self,
            AggOp::Count(_)
                | AggOp::Sum(_)
                | AggOp::Avg(_)
                | AggOp::Min(_)
                | AggOp::Max(_)
                | AggOp::Variance(_)
                | AggOp::StdDev(_)
                | AggOp::Ratio(_)
        )
    }

    /// Update state with one event row. Dispatches to the concrete per-op impl.
    ///
    /// - `field`: the field to aggregate over (None for Count/Ratio and for
    ///   buffer/geo ops which carry their own field refs in state)
    /// - `where_matched`: pre-evaluated predicate result (the apply path wires this
    ///   from an Expr evaluator; here callers set it directly)
    pub fn update(&mut self, row: &Row, now_ms: i64, field: Option<&str>, where_matched: bool) {
        match self {
            AggOp::Count(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Sum(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Avg(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Min(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Max(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Variance(s) => s.update(row, now_ms, field, where_matched),
            AggOp::StdDev(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Ratio(s) => s.update(row, now_ms, field, where_matched),
            AggOp::CountDistinct(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Percentile(s) => s.update(row, now_ms, field, where_matched),
            AggOp::TopK(s) => s.update(row, now_ms, field, where_matched),
            AggOp::BloomMember(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Entropy(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Windowed(w) => w.update(row, now_ms, field, where_matched),
            // Point/ordinal
            AggOp::First(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Last(s) => s.update(row, now_ms, field, where_matched),
            AggOp::FirstN(s) => s.update(row, now_ms, field, where_matched),
            AggOp::LastN(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Lag(s) => s.update(row, now_ms, field, where_matched),
            // Recency markers
            AggOp::FirstSeen(s)
            | AggOp::LastSeen(s)
            | AggOp::Age(s)
            | AggOp::HasSeen(s)
            | AggOp::TimeSince(s) => s.update(row, now_ms, field, where_matched),
            AggOp::TimeSinceLastN(s) => s.update(row, now_ms, field, where_matched),
            // Streak
            AggOp::Streak(s) | AggOp::MaxStreak(s) => s.update(row, now_ms, field, where_matched),
            AggOp::NegativeStreak(s) => s.update(row, now_ms, field, where_matched),
            // Windowed recency
            AggOp::FirstSeenInWindow(s) => s.update(row, now_ms, field, where_matched),
            // Decay
            AggOp::Ewma(op) => op
                .state
                .update(row, now_ms, field, where_matched, op.half_life_ms),
            AggOp::EwVar(op) => op
                .state
                .update(row, now_ms, field, where_matched, op.half_life_ms),
            AggOp::EwZScore(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.half_life_ms)
            }
            AggOp::DecayedSum(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.half_life_ms)
            }
            AggOp::DecayedCount(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.half_life_ms)
            }
            AggOp::Twa(s) => s.update(row, now_ms, field, where_matched),
            // Velocity
            AggOp::RateOfChange(s) => s.update(row, now_ms, field, where_matched),
            AggOp::InterArrivalStats(s) => s.update(row, now_ms, field, where_matched),
            AggOp::BurstCount(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.sub_window_ms)
            }
            AggOp::DeltaFromPrev(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Trend(s) => s.update(row, now_ms, field, where_matched),
            AggOp::TrendResidual(s) => s.update(row, now_ms, field, where_matched),
            AggOp::OutlierCount(op) => op.state.update(row, now_ms, field, where_matched, op.sigma),
            AggOp::ValueChangeCount(s) => s.update(row, now_ms, field, where_matched),
            AggOp::ZScore(s) => s.update(row, now_ms, field, where_matched),
            // ── Buffer + geo ─────────────────────────────────────────────
            AggOp::Histogram(s) => s.update(row, field, where_matched),
            AggOp::HourOfDayHistogram(s) => s.update(now_ms, where_matched),
            AggOp::DowHourHistogram(s) => s.update(now_ms, where_matched),
            AggOp::SeasonalDeviation(s) => s.update(row, now_ms, field, where_matched),
            AggOp::EventTypeMix(s) => s.update(row, field, where_matched),
            AggOp::MostRecentN(s) => s.update(row, field, where_matched),
            AggOp::ReservoirSample(s) => s.update(row, field, where_matched),
            AggOp::GeoVelocity(s) => s.update(row, now_ms, where_matched),
            AggOp::GeoDistance(s) => s.update(row, where_matched),
            AggOp::GeoSpread(s) => s.update(row, where_matched),
            AggOp::DistanceFromHome(s) => s.update(row, where_matched),
        }
    }

    /// Apply-path entry point: evaluates `where_expr` (if any) before
    /// forwarding to the underlying state's update.
    ///
    /// For Ratio: predicate gates the numerator only; total always increments
    /// unconditionally on every seen row (D-03 semantics).
    /// For all other ops: predicate gates the whole update.
    ///
    /// # SDK-AGG-04
    pub fn update_with_row(
        &mut self,
        row: &Row,
        now_ms: i64,
        field: Option<&str>,
        where_expr: Option<&std::sync::Arc<crate::expr::Expr>>,
    ) {
        let where_matched = match where_expr {
            Some(e) => crate::agg_where::evaluate_where_predicate(e, row),
            None => true,
        };

        match self {
            AggOp::Windowed(w) => {
                // Windowed delegates bucket routing to WindowedOp::update_with_row,
                // which threads the predicate into each bucket's inner AggOp.
                w.update_with_row(row, now_ms, field, where_expr);
            }
            _ => {
                // All other ops (including Ratio): pass where_matched directly.
                // RatioState::update already implements "gate numerator only"
                // semantics — it increments total unconditionally and matching
                // only when where_matched is true.
                self.update(row, now_ms, field, where_matched);
            }
        }
    }

    /// Apply-loop fast-path using a pre-extracted field value.
    ///
    /// This is the hot-path entry point for non-windowed ops. Rather than
    /// calling `Row::get(field)` once per feature (as `update_with_row` does),
    /// the apply loop pre-extracts each distinct field once into `ExtractedFields`
    /// and passes the result here. Field-bearing ops call `update_pre(pre_val, ...)`
    /// on their concrete state; fieldless ops (Count, Ratio, Recency, Streak, etc.)
    /// receive `pre_val = None` and ignore it.
    ///
    /// Windowed ops fall back to `update_with_row` because windowed bucket routing
    /// delegates to `WindowedOp::update_with_row` which still needs `(row, field)`.
    ///
    /// `where_expr` still evaluates against `row` — expression predicates may
    /// reference multiple fields and cannot be pre-extracted in the same way.
    ///
    /// `field_idx` and `extracted` are passed through so `EventTypeMix` can call
    /// its `update_at` fast-path (consuming the pre-extracted Value directly,
    /// avoiding the `row.get(fname)` scan that `update()` pays).
    // reason: pre-extraction protocol threads each independently-meaningful
    // hot-path parameter (pre_val, now_ms, where_expr, row, field, indices)
    // into the inner dispatch; struct-bag refactor would add per-event alloc
    // and obscure call-site intent on the apply hot path.
    #[allow(clippy::too_many_arguments)]
    pub fn update_with_extracted(
        &mut self,
        pre_val: Option<&Value>,
        now_ms: i64,
        where_expr: Option<&std::sync::Arc<crate::expr::Expr>>,
        row: &Row,
        field: Option<&str>,
        field_idx: u8,
        extracted: &ExtractedFields<'_>,
        lat_idx: u8,
        lon_idx: u8,
    ) {
        let where_matched = match where_expr {
            Some(e) => crate::agg_where::evaluate_where_predicate(e, row),
            None => true,
        };
        self.update_with_extracted_no_where(
            pre_val,
            now_ms,
            row,
            field,
            field_idx,
            extracted,
            lat_idx,
            lon_idx,
            where_matched,
        );
    }

    /// Pre-where-evaluated sibling of `update_with_extracted`.
    ///
    /// Body is identical to `update_with_extracted` minus the `where_expr`
    /// evaluation block — the caller has already computed `where_matched`
    /// and threads it down. Used by:
    ///   1. `update_with_extracted` (after evaluating `where_expr` once at the
    ///      outer dispatcher).
    ///   2. `WindowedOp::update_at` (per-bucket dispatch — `where_matched` was
    ///      computed once at the outer level; per-bucket re-evaluation is
    ///      forbidden — predicate evaluation must happen exactly once per event).
    ///
    /// The Windowed arm here calls `WindowedOp::update_at`, NOT `update_with_row`,
    /// so the pre-extraction protocol crosses the WindowedOp wrapper boundary.
    // reason: pre-where-evaluated sibling of update_with_extracted; same
    // hot-path parameter list with where_matched threaded in pre-computed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_with_extracted_no_where(
        &mut self,
        pre_val: Option<&Value>,
        now_ms: i64,
        row: &Row,
        field: Option<&str>,
        field_idx: u8,
        extracted: &ExtractedFields<'_>,
        lat_idx: u8,
        lon_idx: u8,
        where_matched: bool,
    ) {
        match self {
            // Windowed dispatches to update_at (NOT update_with_row): the
            // pre-extraction protocol crosses the WindowedOp wrapper boundary.
            // pre_val is threaded through (already resolved by the outer
            // apply-loop via the agg-local → union-index remap); the windowed
            // arm must NOT re-extract using field_idx, which is agg-local.
            AggOp::Windowed(w) => {
                w.update_at(
                    pre_val,
                    extracted,
                    field_idx,
                    lat_idx,
                    lon_idx,
                    now_ms,
                    where_matched,
                );
            }
            // Fieldless ops: pre_val is None, where_matched gates the update.
            AggOp::Count(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Ratio(s) => s.update(row, now_ms, field, where_matched),
            AggOp::FirstSeen(s)
            | AggOp::LastSeen(s)
            | AggOp::Age(s)
            | AggOp::HasSeen(s)
            | AggOp::TimeSince(s) => s.update(row, now_ms, field, where_matched),
            AggOp::TimeSinceLastN(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Streak(s) | AggOp::MaxStreak(s) => s.update(row, now_ms, field, where_matched),
            AggOp::NegativeStreak(s) => s.update(row, now_ms, field, where_matched),
            AggOp::FirstSeenInWindow(s) => s.update(row, now_ms, field, where_matched),
            // Geo ops: index-based fast path when lat_idx is resolved; fall
            // back to row-based update when lat_idx == FIELD_IDX_NONE.
            AggOp::GeoVelocity(s) => {
                if lat_idx != FIELD_IDX_NONE {
                    s.update_at(extracted, lat_idx, lon_idx, now_ms, where_matched)
                } else {
                    s.update(row, now_ms, where_matched)
                }
            }
            AggOp::GeoDistance(s) => {
                if lat_idx != FIELD_IDX_NONE {
                    s.update_at(extracted, lat_idx, lon_idx, where_matched)
                } else {
                    s.update(row, where_matched)
                }
            }
            AggOp::GeoSpread(s) => {
                if lat_idx != FIELD_IDX_NONE {
                    s.update_at(extracted, lat_idx, lon_idx, where_matched)
                } else {
                    s.update(row, where_matched)
                }
            }
            AggOp::DistanceFromHome(s) => {
                if lat_idx != FIELD_IDX_NONE {
                    s.update_at(extracted, lat_idx, lon_idx, where_matched)
                } else {
                    s.update(row, where_matched)
                }
            }
            // Histogram ops: no field, time-based.
            AggOp::HourOfDayHistogram(s) => s.update(now_ms, where_matched),
            AggOp::DowHourHistogram(s) => s.update(now_ms, where_matched),
            // Decay / velocity ops with field access — pre_val path.
            AggOp::Ewma(op) => op
                .state
                .update(row, now_ms, field, where_matched, op.half_life_ms),
            AggOp::EwVar(op) => op
                .state
                .update(row, now_ms, field, where_matched, op.half_life_ms),
            AggOp::EwZScore(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.half_life_ms)
            }
            AggOp::DecayedSum(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.half_life_ms)
            }
            AggOp::DecayedCount(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.half_life_ms)
            }
            AggOp::Twa(s) => s.update(row, now_ms, field, where_matched),
            AggOp::RateOfChange(s) => s.update(row, now_ms, field, where_matched),
            AggOp::InterArrivalStats(s) => s.update(row, now_ms, field, where_matched),
            AggOp::BurstCount(op) => {
                op.state
                    .update(row, now_ms, field, where_matched, op.sub_window_ms)
            }
            AggOp::DeltaFromPrev(s) => s.update(row, now_ms, field, where_matched),
            AggOp::Trend(s) => s.update(row, now_ms, field, where_matched),
            AggOp::TrendResidual(s) => s.update(row, now_ms, field, where_matched),
            AggOp::OutlierCount(op) => op.state.update(row, now_ms, field, where_matched, op.sigma),
            AggOp::ValueChangeCount(s) => s.update(row, now_ms, field, where_matched),
            AggOp::ZScore(s) => s.update(row, now_ms, field, where_matched),
            AggOp::SeasonalDeviation(s) => s.update(row, now_ms, field, where_matched),
            // Structural outputs using row-based field access — fall back.
            AggOp::Histogram(s) => s.update(row, field, where_matched),
            // EventTypeMix consumes the dispatcher-resolved pre_val — same
            // contract as the Sum/Avg/Min/Max/TopK arms below. Re-extracting
            // from extracted[field_idx] would silently read the wrong slot
            // whenever the agg-local idx differs from the union idx.
            AggOp::EventTypeMix(s) => s.update_at(pre_val, where_matched),
            AggOp::MostRecentN(s) => s.update(row, field, where_matched),
            AggOp::ReservoirSample(s) => s.update(row, field, where_matched),
            // Field-bearing ops with update_pre — hot path.
            AggOp::Sum(s) => s.update_pre(pre_val, where_matched),
            AggOp::Avg(s) => s.update_pre(pre_val, where_matched),
            AggOp::Min(s) => s.update_pre(pre_val, where_matched),
            AggOp::Max(s) => s.update_pre(pre_val, where_matched),
            AggOp::Variance(s) => s.update_pre(pre_val, where_matched),
            AggOp::StdDev(s) => s.update_pre(pre_val, where_matched),
            AggOp::CountDistinct(s) => s.update_pre(pre_val, where_matched),
            AggOp::Percentile(s) => s.update_pre(pre_val, where_matched),
            AggOp::TopK(s) => s.update_pre(pre_val, where_matched),
            AggOp::BloomMember(s) => s.update_pre(pre_val, where_matched),
            AggOp::Entropy(s) => s.update_pre(pre_val, where_matched),
            AggOp::First(s) => s.update_pre(pre_val, where_matched),
            AggOp::Last(s) => s.update_pre(pre_val, where_matched),
            AggOp::FirstN(s) => s.update_pre(pre_val, where_matched),
            AggOp::LastN(s) => s.update_pre(pre_val, where_matched),
            AggOp::Lag(s) => s.update_pre(pre_val, where_matched),
        }
    }

    /// Query current aggregation value. Dispatches to the concrete per-op impl.
    ///
    /// `query_time_ms` is the query-time event clock (used by WindowedOp to
    /// determine which buckets are active). Lifetime ops ignore it.
    pub fn query(&self, query_time_ms: i64) -> Value {
        match self {
            AggOp::Count(s) => s.query(),
            AggOp::Sum(s) => s.query(),
            AggOp::Avg(s) => s.query(),
            AggOp::Min(s) => s.query(),
            AggOp::Max(s) => s.query(),
            AggOp::Variance(s) => s.query_variance(),
            AggOp::StdDev(s) => s.query_stddev(),
            AggOp::Ratio(s) => s.query(),
            AggOp::CountDistinct(s) => s.query(),
            AggOp::Percentile(s) => s.query(),
            AggOp::TopK(s) => s.query(),
            AggOp::BloomMember(s) => s.query(),
            AggOp::Entropy(s) => s.query(),
            AggOp::Windowed(w) => w.query(query_time_ms),
            // Point/ordinal
            AggOp::First(s) => s.query(),
            AggOp::Last(s) => s.query(),
            AggOp::FirstN(s) => s.query(),
            AggOp::LastN(s) => s.query(),
            AggOp::Lag(s) => s.query(),
            // Recency markers (each variant projects a different aspect of SeenState)
            AggOp::FirstSeen(s) => s.query_first_seen(),
            AggOp::LastSeen(s) => s.query_last_seen(),
            AggOp::Age(s) => s.query_age(query_time_ms),
            AggOp::HasSeen(s) => s.query_has_seen(),
            AggOp::TimeSince(s) => s.query_time_since(query_time_ms),
            AggOp::TimeSinceLastN(s) => s.query(query_time_ms),
            // Streak (Streak returns current; MaxStreak returns max-seen)
            AggOp::Streak(s) => s.query_current(),
            AggOp::MaxStreak(s) => s.query_max(),
            AggOp::NegativeStreak(s) => s.query(),
            // Windowed recency
            AggOp::FirstSeenInWindow(s) => s.query(query_time_ms),
            // Decay
            AggOp::Ewma(op) => op.state.query(),
            AggOp::EwVar(op) => op.state.query_variance(),
            AggOp::EwZScore(op) => op.state.query(),
            AggOp::DecayedSum(op) => op.state.query(),
            AggOp::DecayedCount(op) => op.state.query(),
            AggOp::Twa(s) => s.query(),
            // Velocity
            AggOp::RateOfChange(s) => s.query(),
            AggOp::InterArrivalStats(s) => s.query(),
            AggOp::BurstCount(op) => op.state.query(),
            AggOp::DeltaFromPrev(s) => s.query(),
            AggOp::Trend(s) => s.query(),
            AggOp::TrendResidual(s) => s.query(),
            AggOp::OutlierCount(op) => op.state.query(),
            AggOp::ValueChangeCount(s) => s.query(),
            AggOp::ZScore(s) => s.query(),
            // Buffer + geo
            AggOp::Histogram(s) => s.query(),
            AggOp::HourOfDayHistogram(s) => s.query(),
            AggOp::DowHourHistogram(s) => s.query(),
            AggOp::SeasonalDeviation(s) => s.query(),
            AggOp::EventTypeMix(s) => s.query(),
            AggOp::MostRecentN(s) => s.query(),
            AggOp::ReservoirSample(s) => s.query(),
            AggOp::GeoVelocity(s) => s.query(),
            AggOp::GeoDistance(s) => s.query(),
            AggOp::GeoSpread(s) => s.query(),
            AggOp::DistanceFromHome(s) => s.query(),
        }
    }
}

// `AggKind::supports_windowed_wrap()` is the canonical predicate covering
// core scalar + sketch ops.

// ─── output_type_for ─────────────────────────────────────────────────────────

/// Register-time output type inference.
///
/// Used by the schema propagator to infer the output FieldType of an
/// aggregation feature without running any events.
///
/// Rules:
/// - Count → I64
/// - Sum, Avg, Variance, StdDev, Ratio → F64
/// - Min, Max → inherit the upstream field's FieldType
pub fn output_type_for(
    upstream: &Schema,
    desc: &AggOpDescriptor,
) -> Result<FieldType, AggTypeError> {
    match desc.kind {
        AggKind::Count | AggKind::CountDistinct => Ok(FieldType::I64),
        AggKind::Sum
        | AggKind::Avg
        | AggKind::Variance
        | AggKind::StdDev
        | AggKind::Ratio
        | AggKind::Percentile
        | AggKind::Entropy => Ok(FieldType::F64),
        AggKind::TopK => Ok(FieldType::Json),
        AggKind::BloomMember => Ok(FieldType::Bool),
        AggKind::Min | AggKind::Max | AggKind::First | AggKind::Last | AggKind::Lag => {
            let field = desc
                .field
                .as_deref()
                .ok_or(AggTypeError::FieldRequired { kind: desc.kind })?;
            upstream
                .fields
                .get(field)
                .copied()
                .ok_or_else(|| AggTypeError::FieldMissing {
                    field: field.to_string(),
                })
        }
        // Point ops returning JSON-array string
        AggKind::FirstN | AggKind::LastN => {
            // Validate the field exists for register-time error parity
            let field = desc
                .field
                .as_deref()
                .ok_or(AggTypeError::FieldRequired { kind: desc.kind })?;
            if !upstream.fields.contains_key(field) {
                return Err(AggTypeError::FieldMissing {
                    field: field.to_string(),
                });
            }
            Ok(FieldType::Str)
        }
        // Recency markers
        AggKind::FirstSeen | AggKind::LastSeen => Ok(FieldType::Datetime),
        AggKind::Age | AggKind::TimeSince | AggKind::TimeSinceLastN => Ok(FieldType::I64),
        AggKind::HasSeen | AggKind::FirstSeenInWindow => Ok(FieldType::Bool),
        // Streak family
        AggKind::Streak | AggKind::MaxStreak | AggKind::NegativeStreak => Ok(FieldType::I64),
        // Decay / velocity / z ops emit F64 except integer counters.
        AggKind::Ewma
        | AggKind::EwVar
        | AggKind::EwZScore
        | AggKind::DecayedSum
        | AggKind::DecayedCount
        | AggKind::Twa
        | AggKind::RateOfChange
        | AggKind::InterArrivalStats
        | AggKind::Trend
        | AggKind::TrendResidual
        | AggKind::ZScore => Ok(FieldType::F64),
        AggKind::BurstCount | AggKind::OutlierCount | AggKind::ValueChangeCount => {
            Ok(FieldType::I64)
        }
        AggKind::DeltaFromPrev => {
            // Inherit the upstream field's type per AGG-VEL-04.
            let field = desc
                .field
                .as_deref()
                .ok_or(AggTypeError::FieldRequired { kind: desc.kind })?;
            upstream
                .fields
                .get(field)
                .copied()
                .ok_or_else(|| AggTypeError::FieldMissing {
                    field: field.to_string(),
                })
        }
        // Structured outputs have no FieldType representation — they appear only
        // as aggregation feature outputs (Value::List / Value::Map). The schema
        // propagator treats these as FieldType::Str for placeholder naming
        // (no downstream derivation can consume them as scalars in v0).
        AggKind::Histogram
        | AggKind::HourOfDayHistogram
        | AggKind::DowHourHistogram
        | AggKind::EventTypeMix
        | AggKind::MostRecentN
        | AggKind::ReservoirSample => Ok(FieldType::Str),
        // Scalar buffer/geo outputs
        AggKind::SeasonalDeviation
        | AggKind::GeoVelocity
        | AggKind::GeoDistance
        | AggKind::GeoSpread
        | AggKind::DistanceFromHome => Ok(FieldType::F64),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;
    use std::collections::BTreeMap;

    fn desc(kind: AggKind) -> AggOpDescriptor {
        AggOpDescriptor {
            kind,
            ..Default::default()
        }
    }

    fn desc_field(kind: AggKind, field: &str) -> AggOpDescriptor {
        AggOpDescriptor {
            kind,
            field: Some(field.to_string()),
            ..Default::default()
        }
    }

    fn desc_windowed(kind: AggKind, window_ms: u64) -> AggOpDescriptor {
        AggOpDescriptor {
            kind,
            window_ms: Some(window_ms),
            ..Default::default()
        }
    }

    fn schema_with(pairs: &[(&str, FieldType)]) -> Schema {
        let mut fields = BTreeMap::new();
        for (k, v) in pairs {
            fields.insert(k.to_string(), *v);
        }
        Schema {
            fields,
            optional_fields: vec![],
        }
    }

    // ── AggOp::new dispatch ───────────────────────────────────────────────

    #[test]
    fn aggop_new_dispatches_on_kind() {
        // Lifetime variants
        assert!(matches!(AggOp::new(&desc(AggKind::Count)), AggOp::Count(_)));
        assert!(matches!(AggOp::new(&desc(AggKind::Sum)), AggOp::Sum(_)));
        assert!(matches!(AggOp::new(&desc(AggKind::Avg)), AggOp::Avg(_)));
        assert!(matches!(AggOp::new(&desc(AggKind::Min)), AggOp::Min(_)));
        assert!(matches!(AggOp::new(&desc(AggKind::Max)), AggOp::Max(_)));
        assert!(matches!(
            AggOp::new(&desc(AggKind::Variance)),
            AggOp::Variance(_)
        ));
        assert!(matches!(
            AggOp::new(&desc(AggKind::StdDev)),
            AggOp::StdDev(_)
        ));
        assert!(matches!(AggOp::new(&desc(AggKind::Ratio)), AggOp::Ratio(_)));

        // Windowed variant when window_ms is set
        assert!(matches!(
            AggOp::new(&desc_windowed(AggKind::Count, 60_000)),
            AggOp::Windowed(_)
        ));
    }

    // ── AggOp::query dispatch ─────────────────────────────────────────────

    #[test]
    fn aggop_query_dispatches_on_variant() {
        let r = Row::new().with_field("x", Value::F64(5.0));
        let t = 0_i64;

        // Count: 1 event → I64(1)
        let mut count = AggOp::Count(CountState::default());
        count.update(&r, t, None, true);
        assert_eq!(count.query(t), Value::I64(1));

        // Sum: 5.0 → F64(5.0)
        let mut sum = AggOp::Sum(SumState::default());
        sum.update(&r, t, Some("x"), true);
        assert_eq!(sum.query(t), Value::F64(5.0));

        // Avg: 5.0 → F64(5.0)
        let mut avg = AggOp::Avg(AvgState::default());
        avg.update(&r, t, Some("x"), true);
        assert_eq!(avg.query(t), Value::F64(5.0));

        // Min: 5.0 → F64(5.0)
        let mut min_op = AggOp::Min(MinState::default());
        min_op.update(&r, t, Some("x"), true);
        assert_eq!(min_op.query(t), Value::F64(5.0));

        // Max: 5.0 → F64(5.0)
        let mut max_op = AggOp::Max(MaxState::default());
        max_op.update(&r, t, Some("x"), true);
        assert_eq!(max_op.query(t), Value::F64(5.0));

        // Variance: single element → Null
        let mut var = AggOp::Variance(VarianceState::default());
        var.update(&r, t, Some("x"), true);
        assert_eq!(var.query(t), Value::Null);

        // StdDev: single element → Null
        let mut sd = AggOp::StdDev(VarianceState::default());
        sd.update(&r, t, Some("x"), true);
        assert_eq!(sd.query(t), Value::Null);

        // Ratio: 1 matched out of 1 → F64(1.0)
        let mut ratio = AggOp::Ratio(RatioState::default());
        ratio.update(&r, t, None, true);
        assert_eq!(ratio.query(t), Value::F64(1.0));
    }

    // ── output_type_for ───────────────────────────────────────────────────

    #[test]
    fn output_type_for_count_returns_i64() {
        let s = schema_with(&[("amount", FieldType::F64)]);
        assert_eq!(
            output_type_for(&s, &desc(AggKind::Count)),
            Ok(FieldType::I64)
        );
    }

    #[test]
    fn output_type_for_sum_avg_returns_f64() {
        let s = schema_with(&[("amount", FieldType::F64)]);
        assert_eq!(
            output_type_for(&s, &desc_field(AggKind::Sum, "amount")),
            Ok(FieldType::F64)
        );
        assert_eq!(
            output_type_for(&s, &desc_field(AggKind::Avg, "amount")),
            Ok(FieldType::F64)
        );
    }

    #[test]
    fn output_type_for_min_max_inherits_field_type() {
        let s = schema_with(&[("amount", FieldType::F64), ("count", FieldType::I64)]);
        assert_eq!(
            output_type_for(&s, &desc_field(AggKind::Min, "amount")),
            Ok(FieldType::F64)
        );
        assert_eq!(
            output_type_for(&s, &desc_field(AggKind::Max, "count")),
            Ok(FieldType::I64)
        );
    }

    #[test]
    fn output_type_for_min_missing_field_returns_error() {
        let s = schema_with(&[("amount", FieldType::F64)]);
        let result = output_type_for(&s, &desc_field(AggKind::Min, "nonexistent"));
        assert_eq!(
            result,
            Err(AggTypeError::FieldMissing {
                field: "nonexistent".to_string()
            })
        );
    }

    #[test]
    fn output_type_for_min_no_field_returns_field_required_error() {
        let s = schema_with(&[("amount", FieldType::F64)]);
        let result = output_type_for(&s, &desc(AggKind::Min));
        assert_eq!(
            result,
            Err(AggTypeError::FieldRequired { kind: AggKind::Min })
        );
    }

    // ── update_with_row tests (Plan 05-02) ────────────────────────────────

    fn make_where_expr(src: &str) -> std::sync::Arc<crate::expr::Expr> {
        std::sync::Arc::new(crate::expr::parse(src).expect("should parse where expr"))
    }

    fn row_amount(v: f64) -> Row {
        Row::new().with_field("amount", Value::F64(v))
    }

    fn row_status(s: &str) -> Row {
        Row::new().with_field("status", Value::Str(s.into()))
    }

    /// 5 rows, 3 match predicate "(amount > 100)" → Count == I64(3).
    #[test]
    fn count_with_where_true_increments() {
        let where_expr = make_where_expr("(amount > 100)");
        let mut op = AggOp::Count(CountState::default());
        let amounts = [50.0, 150.0, 200.0, 80.0, 300.0]; // 3 > 100
        for &a in &amounts {
            op.update_with_row(&row_amount(a), 0, None, Some(&where_expr));
        }
        assert_eq!(op.query(0), Value::I64(3), "only 3 rows match amount > 100");
    }

    /// 5 rows, all fail predicate → Count == I64(0).
    #[test]
    fn count_with_where_false_does_not_increment() {
        let where_expr = make_where_expr("(amount > 1000)");
        let mut op = AggOp::Count(CountState::default());
        for &a in &[10.0_f64, 20.0, 30.0, 40.0, 50.0] {
            op.update_with_row(&row_amount(a), 0, None, Some(&where_expr));
        }
        assert_eq!(op.query(0), Value::I64(0), "no rows match amount > 1000");
    }

    /// 5 rows amounts [10,20,30,40,50] with predicate "(amount > 25)" → sum == 120.0.
    #[test]
    fn sum_with_where_skips_non_matching() {
        let where_expr = make_where_expr("(amount > 25)");
        let mut op = AggOp::Sum(SumState::default());
        for &a in &[10.0_f64, 20.0, 30.0, 40.0, 50.0] {
            op.update_with_row(&row_amount(a), 0, Some("amount"), Some(&where_expr));
        }
        // 30 + 40 + 50 = 120
        match op.query(0) {
            Value::F64(v) => assert!((v - 120.0).abs() < 1e-10, "sum should be 120.0, got {v}"),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    /// Ratio "gate numerator only": predicate "(status == 'ok')", 10 events (3 match)
    /// → ratio == 0.3 (matching=3, total=10). D-03 semantics: total always increments.
    #[test]
    fn ratio_with_where_gates_numerator_only() {
        let where_expr = make_where_expr("(status == 'ok')");
        let mut op = AggOp::Ratio(RatioState::default());
        // 3 "ok" rows, 7 other rows
        for i in 0..10 {
            let row = if i < 3 {
                row_status("ok")
            } else {
                row_status("fail")
            };
            op.update_with_row(&row, 0, None, Some(&where_expr));
        }
        match op.query(0) {
            Value::F64(v) => assert!((v - 0.3).abs() < 1e-10, "ratio should be 3/10=0.3, got {v}"),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    /// where_expr=None → always updates, identical to Plan 05-01 update (regression guard).
    #[test]
    fn update_with_none_where_always_updates() {
        let mut op = AggOp::Count(CountState::default());
        let r = Row::new();
        for _ in 0..5 {
            op.update_with_row(&r, 0, None, None);
        }
        assert_eq!(
            op.query(0),
            Value::I64(5),
            "None where_expr → all 5 rows counted"
        );
    }

    // ── Determinism guard ─────────────────────────────────────────────────

    // AggKind has 13 variants (8 core + 5 sketch).
    #[test]
    fn agg_kind_has_sketch_variants() {
        use AggKind::*;
        let _all = [
            Count,
            Sum,
            Avg,
            Min,
            Max,
            Variance,
            StdDev,
            Ratio,
            CountDistinct,
            Percentile,
            TopK,
            BloomMember,
            Entropy,
        ];
        assert_eq!(_all.len(), 13);
    }

    #[test]
    fn no_systemtime_now_in_apply() {
        // Split forbidden patterns so this file does not itself trigger the check.
        let forbidden_clock = ["SystemTime", "::", "now"].concat();
        let forbidden_rand = ["rand", "::"].concat();
        let src = include_str!("agg_op.rs");
        assert!(
            !src.contains(forbidden_clock.as_str()),
            "agg_op.rs must not use wall-clock reads (D-06 determinism invariant)"
        );
        assert!(
            !src.contains(forbidden_rand.as_str()),
            "agg_op.rs must not use rand crate (D-06 determinism invariant)"
        );
    }

    // ExtractedFields inline cap must hold fraud-team's per-source union
    // (~12 fields, max 16 with headroom) without spilling to the heap.
    // Earlier SmallVec[8] sizing spilled on every Txn event, costing
    // ~530 ns/event of allocator traffic.
    #[test]
    fn extracted_fields_inline_cap_holds_fraud_team_union_size() {
        let mut e: super::ExtractedFields<'static> = Default::default();
        for _ in 0..12 {
            e.push(None);
        }
        assert!(
            !e.spilled(),
            "ExtractedFields spilled at len=12; inline cap is too small for fraud-team union (need >=12, ideally 16)."
        );
    }

    #[test]
    fn extracted_fields_inline_cap_at_least_16() {
        // Stronger invariant: cap is >= 16 (matches per-source union sizing).
        let mut e: super::ExtractedFields<'static> = Default::default();
        for _ in 0..16 {
            e.push(None);
        }
        assert!(
            !e.spilled(),
            "ExtractedFields spilled at len=16; inline cap is < 16."
        );
    }
}
