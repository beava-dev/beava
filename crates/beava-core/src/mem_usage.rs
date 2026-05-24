//! Deterministic structural memory accounting for aggregation state.
//!
//! This is intentionally not allocator instrumentation. It reports stable,
//! platform-independent estimates from owned state shape: inline stack slots,
//! `Box<T>` payloads, vector capacity, sketch backing stores, and documented
//! map overhead estimates.

use crate::agg_op::AggOp;
use crate::agg_state::{
    BloomMemberStateWrap, CountDistinctStateWrap, EntropyStateWrap, PercentileStateWrap,
    TopKStateWrap,
};
use crate::row::Value;
use crate::sketches::cms::TopKValue;
use crate::sketches::count_distinct::CountDistinctState;
use crate::sketches::percentile::PercentileState;
use crate::sketches::top_k::TopKState;
use serde::Serialize;
use std::collections::VecDeque;
use std::mem::{size_of, size_of_val};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemBreakdown {
    pub label: String,
    pub bytes: usize,
    pub kind: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemProfile {
    pub label: String,
    pub stack_bytes: usize,
    pub enum_slot_bytes: usize,
    pub payload_bytes: usize,
    pub slack_bytes: usize,
    pub heap_bytes: usize,
    pub breakdown: Vec<MemBreakdown>,
}

impl MemProfile {
    pub fn new(label: impl Into<String>, stack_bytes: usize) -> Self {
        Self {
            label: label.into(),
            stack_bytes,
            enum_slot_bytes: stack_bytes,
            payload_bytes: stack_bytes,
            slack_bytes: 0,
            heap_bytes: 0,
            breakdown: Vec::new(),
        }
    }

    pub fn with_stack_composition(mut self, enum_slot_bytes: usize, payload_bytes: usize) -> Self {
        self.stack_bytes = enum_slot_bytes;
        self.enum_slot_bytes = enum_slot_bytes;
        self.payload_bytes = payload_bytes;
        self.slack_bytes = enum_slot_bytes.saturating_sub(payload_bytes);
        self
    }

    pub fn total_bytes(&self) -> usize {
        self.stack_bytes + self.heap_bytes
    }

    pub fn add_breakdown(
        &mut self,
        label: impl Into<String>,
        bytes: usize,
        kind: impl Into<String>,
        note: impl Into<String>,
    ) {
        if bytes == 0 {
            return;
        }
        self.heap_bytes = self.heap_bytes.saturating_add(bytes);
        self.breakdown.push(MemBreakdown {
            label: label.into(),
            bytes,
            kind: kind.into(),
            note: note.into(),
        });
    }

    pub fn absorb_nested(&mut self, prefix: &str, nested: MemProfile) {
        self.add_breakdown(
            format!("{prefix} stack payload"),
            nested.stack_bytes,
            "nested_stack",
            "nested AggOp stored inside an owned heap allocation",
        );
        for entry in nested.breakdown {
            self.add_breakdown(
                format!("{prefix} / {}", entry.label),
                entry.bytes,
                entry.kind,
                entry.note,
            );
        }
    }
}

pub trait MemUsage {
    fn mem_profile(&self) -> MemProfile;
}

pub fn sort_profiles_desc(rows: &mut [MemProfile]) {
    rows.sort_by(|a, b| {
        b.total_bytes()
            .cmp(&a.total_bytes())
            .then_with(|| a.label.cmp(&b.label))
    });
}

pub fn vec_heap_bytes<T>(vec: &Vec<T>) -> usize {
    vec.capacity().saturating_mul(size_of::<T>())
}

pub fn vecdeque_heap_bytes<T>(deque: &VecDeque<T>) -> usize {
    deque.capacity().saturating_mul(size_of::<T>())
}

pub fn serialized_heap_estimate<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0)
}

pub fn estimated_btree_map_heap_bytes(len: usize, key_value_bytes: usize) -> usize {
    const BTREE_ENTRY_OVERHEAD: usize = 32;
    len.saturating_mul(key_value_bytes.saturating_add(BTREE_ENTRY_OVERHEAD))
}

pub fn estimated_hash_map_heap_bytes(capacity: usize, key_value_bytes: usize) -> usize {
    const HASH_ENTRY_OVERHEAD: usize = 24;
    capacity.saturating_mul(key_value_bytes.saturating_add(HASH_ENTRY_OVERHEAD))
}

#[cfg(test)]
fn serde_profile<T: Serialize>(label: &str, value: &T) -> MemProfile {
    let mut profile = MemProfile::new(label, size_of_val(value));
    profile.add_breakdown(
        format!("{label} serialized owned state estimate"),
        serialized_heap_estimate(value),
        "estimate",
        "deterministic serialized-size proxy for private owned heap fields",
    );
    profile
}

fn value_vec_breakdown(profile: &mut MemProfile, label: &str, values: &Vec<Value>) {
    profile.add_breakdown(
        label,
        vec_heap_bytes(values),
        "Vec",
        "capacity * size_of::<Value>()",
    );
}

fn value_deque_breakdown(profile: &mut MemProfile, label: &str, values: &VecDeque<Value>) {
    profile.add_breakdown(
        label,
        vecdeque_heap_bytes(values),
        "VecDeque",
        "capacity * size_of::<Value>()",
    );
}

fn add_box_allocation(
    profile: &mut MemProfile,
    label: impl Into<String>,
    bytes: usize,
    note: impl Into<String>,
) {
    profile.add_breakdown(label, bytes, "Box", note);
}

fn add_string_breakdown(profile: &mut MemProfile, label: impl Into<String>, value: &String) {
    profile.add_breakdown(
        label,
        value.capacity(),
        "String",
        "capacity bytes for owned string buffer",
    );
}

fn add_count_distinct_breakdown(profile: &mut MemProfile, state: &CountDistinctStateWrap) {
    add_box_allocation(
        profile,
        "Box<CountDistinctStateWrap>",
        size_of_val(state),
        "heap allocation for boxed CountDistinct wrapper",
    );
    match &state.inner {
        CountDistinctState::ExactArray { values } => profile.add_breakdown(
            "CountDistinct exact-array values",
            vec_heap_bytes(values),
            "Vec",
            "capacity * size_of::<u64>() for exact distinct hashes",
        ),
        CountDistinctState::HashSet { .. } => profile.add_breakdown(
            "CountDistinct hash-set slots",
            state
                .inner
                .hash_set_capacity()
                .unwrap_or(0)
                .saturating_mul(16),
            "HashSet",
            "estimated hashbrown slot cost for u64 distinct hashes",
        ),
        CountDistinctState::Hll { sketch } => profile.add_breakdown(
            "CountDistinct HLL registers",
            sketch.register_capacity().saturating_mul(size_of::<u8>()),
            "Vec",
            "capacity * size_of::<u8>() for dense HLL registers",
        ),
    }
}

fn add_percentile_breakdown(profile: &mut MemProfile, state: &PercentileStateWrap) {
    add_box_allocation(
        profile,
        "Box<PercentileStateWrap>",
        size_of_val(state),
        "heap allocation for boxed Percentile wrapper",
    );
    match &state.inner {
        PercentileState::Exact { values, .. } => profile.add_breakdown(
            "Percentile exact samples",
            vec_heap_bytes(values),
            "Vec",
            "capacity * size_of::<f64>() for exact percentile samples",
        ),
        PercentileState::Sketch { sketch } => {
            profile.add_breakdown(
                "UDDSketch positive buckets",
                sketch
                    .positive_bucket_capacity()
                    .saturating_mul(size_of::<(i32, u64)>()),
                "Vec",
                "capacity * size_of::<(i32, u64)>() for positive UDDSketch buckets",
            );
            profile.add_breakdown(
                "UDDSketch negative buckets",
                sketch
                    .negative_bucket_capacity()
                    .saturating_mul(size_of::<(i32, u64)>()),
                "Vec",
                "capacity * size_of::<(i32, u64)>() for negative UDDSketch buckets",
            );
        }
    }
}

fn add_top_k_breakdown(profile: &mut MemProfile, state: &TopKStateWrap) {
    add_box_allocation(
        profile,
        "Box<TopKStateWrap>",
        size_of_val(state),
        "heap allocation for boxed TopK wrapper",
    );
    match &state.inner {
        TopKState::Exact { counts, .. } => profile.add_breakdown(
            "TopK exact BTreeMap entries",
            estimated_btree_map_heap_bytes(counts.len(), 64),
            "BTreeMap",
            "estimated node overhead plus TopKValue/u64 payloads",
        ),
        TopKState::Hybrid { cms, heap, .. } => {
            profile.add_breakdown(
                "TopK count-min counters",
                cms.counter_capacity().saturating_mul(size_of::<i64>()),
                "Vec",
                "capacity * size_of::<i64>() for count-min sketch counters",
            );
            profile.add_breakdown(
                "TopK heap entries",
                heap.heap_capacity()
                    .saturating_mul(size_of::<(u64, TopKValue)>()),
                "Vec",
                "capacity * size_of::<(u64, TopKValue)>() for bounded top-k heap entries",
            );
            profile.add_breakdown(
                "TopK heap index map",
                estimated_hash_map_heap_bytes(
                    heap.index_capacity_estimate(),
                    size_of::<(TopKValue, usize)>(),
                ),
                "AHashMap",
                "estimated slot cost for TopK heap-position side index",
            );
        }
    }
}

fn add_bloom_breakdown(profile: &mut MemProfile, state: &BloomMemberStateWrap) {
    add_box_allocation(
        profile,
        "Box<BloomMemberStateWrap>",
        size_of_val(state),
        "heap allocation for boxed Bloom wrapper",
    );
    profile.add_breakdown(
        "Bloom filter words",
        state.inner.word_capacity().saturating_mul(size_of::<u64>()),
        "Vec",
        "capacity * size_of::<u64>() for bloom bit-array storage",
    );
}

fn add_entropy_breakdown(profile: &mut MemProfile, state: &EntropyStateWrap) {
    add_box_allocation(
        profile,
        "Box<EntropyStateWrap>",
        size_of_val(state),
        "heap allocation for boxed Entropy wrapper",
    );
    profile.add_breakdown(
        "Entropy category map entries",
        estimated_btree_map_heap_bytes(state.inner.category_count(), 48),
        "BTreeMap",
        "estimated node overhead plus String/u64 category payloads",
    );
    profile.add_breakdown(
        "Entropy category string capacity",
        state.inner.key_capacity_bytes(),
        "String",
        "sum of tracked category string capacities",
    );
}

impl MemUsage for AggOp {
    fn mem_profile(&self) -> MemProfile {
        let enum_slot_bytes = size_of::<AggOp>();
        let mut profile = MemProfile::new(aggop_label(self), enum_slot_bytes)
            .with_stack_composition(enum_slot_bytes, aggop_payload_bytes(self));
        match self {
            AggOp::Count(_)
            | AggOp::Sum(_)
            | AggOp::Avg(_)
            | AggOp::Min(_)
            | AggOp::Max(_)
            | AggOp::Variance(_)
            | AggOp::StdDev(_)
            | AggOp::Ratio(_)
            | AggOp::First(_)
            | AggOp::Last(_)
            | AggOp::FirstSeen(_)
            | AggOp::LastSeen(_)
            | AggOp::Age(_)
            | AggOp::HasSeen(_)
            | AggOp::TimeSince(_)
            | AggOp::Streak(_)
            | AggOp::MaxStreak(_)
            | AggOp::NegativeStreak(_)
            | AggOp::FirstSeenInWindow(_)
            | AggOp::Ewma(_)
            | AggOp::EwVar(_)
            | AggOp::EwZScore(_)
            | AggOp::DecayedSum(_)
            | AggOp::DecayedCount(_)
            | AggOp::Twa(_)
            | AggOp::RateOfChange(_)
            | AggOp::InterArrivalStats(_)
            | AggOp::DeltaFromPrev(_)
            | AggOp::Trend(_)
            | AggOp::TrendResidual(_)
            | AggOp::OutlierCount(_)
            | AggOp::ValueChangeCount(_)
            | AggOp::ZScore(_) => {}

            AggOp::FirstN(s) => value_vec_breakdown(&mut profile, "FirstN values", &s.values),
            AggOp::LastN(s) => value_deque_breakdown(&mut profile, "LastN values", &s.values),
            AggOp::Lag(s) => value_deque_breakdown(&mut profile, "Lag values", &s.values),
            AggOp::TimeSinceLastN(s) => profile.add_breakdown(
                "TimeSinceLastN timestamps",
                vecdeque_heap_bytes(&s.times_ms),
                "VecDeque",
                "capacity * size_of::<i64>()",
            ),
            AggOp::BurstCount(op) => {
                profile.add_breakdown(
                    "BurstCount buckets",
                    vec_heap_bytes(&op.state.buckets),
                    "Vec",
                    "capacity * size_of::<u64>()",
                );
                profile.add_breakdown(
                    "BurstCount bucket_epoch",
                    vec_heap_bytes(&op.state.bucket_epoch),
                    "Vec",
                    "capacity * size_of::<i64>()",
                );
            }
            AggOp::Histogram(s) => {
                profile.add_breakdown(
                    "Histogram split points",
                    vec_heap_bytes(&s.buckets),
                    "Vec",
                    "capacity * size_of::<f64>()",
                );
                profile.add_breakdown(
                    "Histogram counts",
                    vec_heap_bytes(&s.counts),
                    "Vec",
                    "capacity * size_of::<u64>()",
                );
            }
            AggOp::DowHourHistogram(s) => profile.add_breakdown(
                "DowHourHistogram counts",
                vec_heap_bytes(&s.counts),
                "Vec",
                "capacity * size_of::<u64>()",
            ),
            AggOp::MostRecentN(s) => {
                value_vec_breakdown(&mut profile, "MostRecentN buffer", &s.buf)
            }
            AggOp::ReservoirSample(s) => {
                value_vec_breakdown(&mut profile, "ReservoirSample reservoir", &s.reservoir)
            }
            AggOp::EventTypeMix(s) => {
                let boxed_bytes = size_of_val(&**s);
                profile.add_breakdown(
                    "Box<EventTypeMixState>",
                    boxed_bytes,
                    "Box",
                    "heap allocation for boxed payload",
                );
                profile.add_breakdown(
                    "EventTypeMix BTreeMap entries",
                    estimated_btree_map_heap_bytes(s.counts.len(), 48),
                    "BTreeMap",
                    "estimated node overhead plus String/u64 payload",
                );
                if let Some(allowed) = &s.allowed {
                    profile.add_breakdown(
                        "EventTypeMix allowed categories",
                        vec_heap_bytes(allowed),
                        "Vec",
                        "capacity * size_of::<String>()",
                    );
                }
            }
            AggOp::HourOfDayHistogram(s) => add_box_allocation(
                &mut profile,
                "Box<HourOfDayHistogramState>",
                size_of_val(&**s),
                "heap allocation for fixed inline hour-of-day counts",
            ),
            AggOp::SeasonalDeviation(s) => add_box_allocation(
                &mut profile,
                "Box<SeasonalDeviationState>",
                size_of_val(&**s),
                "heap allocation for fixed inline per-hour buckets",
            ),
            AggOp::GeoVelocity(s) => {
                add_box_allocation(
                    &mut profile,
                    "Box<GeoVelocityState>",
                    size_of_val(&**s),
                    "heap allocation for boxed payload",
                );
                add_string_breakdown(&mut profile, "GeoVelocity lat_field", &s.lat_field);
                add_string_breakdown(&mut profile, "GeoVelocity lon_field", &s.lon_field);
            }
            AggOp::GeoDistance(s) => {
                add_box_allocation(
                    &mut profile,
                    "Box<GeoDistanceState>",
                    size_of_val(&**s),
                    "heap allocation for boxed payload",
                );
                add_string_breakdown(&mut profile, "GeoDistance lat_field", &s.lat_field);
                add_string_breakdown(&mut profile, "GeoDistance lon_field", &s.lon_field);
            }
            AggOp::GeoSpread(s) => {
                add_box_allocation(
                    &mut profile,
                    "Box<GeoSpreadState>",
                    size_of_val(&**s),
                    "heap allocation for boxed payload",
                );
                add_string_breakdown(&mut profile, "GeoSpread lat_field", &s.lat_field);
                add_string_breakdown(&mut profile, "GeoSpread lon_field", &s.lon_field);
            }
            AggOp::DistanceFromHome(s) => {
                profile.add_breakdown(
                    "Box<DistanceFromHomeState>",
                    size_of_val(&**s),
                    "Box",
                    "heap allocation for boxed payload",
                );
                profile.add_breakdown(
                    "DistanceFromHome coordinate buffer",
                    vec_heap_bytes(&s.buf),
                    "Vec",
                    "capacity * size_of::<(f64, f64)>()",
                );
            }

            AggOp::CountDistinct(s) => add_count_distinct_breakdown(&mut profile, s),
            AggOp::Percentile(s) => add_percentile_breakdown(&mut profile, s),
            AggOp::TopK(s) => add_top_k_breakdown(&mut profile, s),
            AggOp::BloomMember(s) => add_bloom_breakdown(&mut profile, s),
            AggOp::Entropy(s) => add_entropy_breakdown(&mut profile, s),

            AggOp::Windowed(w) => {
                profile.add_breakdown(
                    "Box<WindowedOp>",
                    size_of_val(&**w),
                    "Box",
                    "heap allocation for boxed WindowedOp payload",
                );
                if w.buckets.spilled() {
                    profile.add_breakdown(
                        "WindowedOp spilled bucket SmallVec",
                        w.buckets
                            .capacity()
                            .saturating_mul(size_of::<(i64, Box<AggOp>)>()),
                        "SmallVec",
                        "spilled capacity * size_of::<(i64, Box<AggOp>)>()",
                    );
                }
                for (idx, (_, bucket)) in w.buckets.iter().enumerate() {
                    let nested = bucket.mem_profile();
                    profile.add_breakdown(
                        format!("Windowed bucket {idx} Box<AggOp>"),
                        size_of::<AggOp>(),
                        "Box",
                        "heap allocation for bucket AggOp enum slot",
                    );
                    for entry in nested.breakdown {
                        profile.add_breakdown(
                            format!("Windowed bucket {idx} / {}", entry.label),
                            entry.bytes,
                            entry.kind,
                            entry.note,
                        );
                    }
                }
            }
        }
        profile
    }
}

fn aggop_payload_bytes(op: &AggOp) -> usize {
    match op {
        AggOp::Count(s) => size_of_val(s),
        AggOp::Sum(s) => size_of_val(s),
        AggOp::Avg(s) => size_of_val(s),
        AggOp::Min(s) => size_of_val(s),
        AggOp::Max(s) => size_of_val(s),
        AggOp::Variance(s) => size_of_val(s),
        AggOp::StdDev(s) => size_of_val(s),
        AggOp::Ratio(s) => size_of_val(s),
        AggOp::CountDistinct(s) => size_of_val(s),
        AggOp::Percentile(s) => size_of_val(s),
        AggOp::TopK(s) => size_of_val(s),
        AggOp::BloomMember(s) => size_of_val(s),
        AggOp::Entropy(s) => size_of_val(s),
        AggOp::Windowed(s) => size_of_val(s),
        AggOp::First(s) => size_of_val(s),
        AggOp::Last(s) => size_of_val(s),
        AggOp::FirstN(s) => size_of_val(s),
        AggOp::LastN(s) => size_of_val(s),
        AggOp::Lag(s) => size_of_val(s),
        AggOp::FirstSeen(s) => size_of_val(s),
        AggOp::LastSeen(s) => size_of_val(s),
        AggOp::Age(s) => size_of_val(s),
        AggOp::HasSeen(s) => size_of_val(s),
        AggOp::TimeSince(s) => size_of_val(s),
        AggOp::TimeSinceLastN(s) => size_of_val(s),
        AggOp::Streak(s) => size_of_val(s),
        AggOp::MaxStreak(s) => size_of_val(s),
        AggOp::NegativeStreak(s) => size_of_val(s),
        AggOp::FirstSeenInWindow(s) => size_of_val(s),
        AggOp::Ewma(s) => size_of_val(s),
        AggOp::EwVar(s) => size_of_val(s),
        AggOp::EwZScore(s) => size_of_val(s),
        AggOp::DecayedSum(s) => size_of_val(s),
        AggOp::DecayedCount(s) => size_of_val(s),
        AggOp::Twa(s) => size_of_val(s),
        AggOp::RateOfChange(s) => size_of_val(s),
        AggOp::InterArrivalStats(s) => size_of_val(s),
        AggOp::BurstCount(s) => size_of_val(s),
        AggOp::DeltaFromPrev(s) => size_of_val(s),
        AggOp::Trend(s) => size_of_val(s),
        AggOp::TrendResidual(s) => size_of_val(s),
        AggOp::OutlierCount(s) => size_of_val(s),
        AggOp::ValueChangeCount(s) => size_of_val(s),
        AggOp::ZScore(s) => size_of_val(s),
        AggOp::Histogram(s) => size_of_val(s),
        AggOp::HourOfDayHistogram(s) => size_of_val(s),
        AggOp::DowHourHistogram(s) => size_of_val(s),
        AggOp::SeasonalDeviation(s) => size_of_val(s),
        AggOp::EventTypeMix(s) => size_of_val(s),
        AggOp::MostRecentN(s) => size_of_val(s),
        AggOp::ReservoirSample(s) => size_of_val(s),
        AggOp::GeoVelocity(s) => size_of_val(s),
        AggOp::GeoDistance(s) => size_of_val(s),
        AggOp::GeoSpread(s) => size_of_val(s),
        AggOp::DistanceFromHome(s) => size_of_val(s),
    }
}

fn aggop_label(op: &AggOp) -> String {
    match op {
        AggOp::Count(_) => "Count",
        AggOp::Sum(_) => "Sum",
        AggOp::Avg(_) => "Avg",
        AggOp::Min(_) => "Min",
        AggOp::Max(_) => "Max",
        AggOp::Variance(_) => "Variance",
        AggOp::StdDev(_) => "StdDev",
        AggOp::Ratio(_) => "Ratio",
        AggOp::CountDistinct(_) => "CountDistinct",
        AggOp::Percentile(_) => "Percentile",
        AggOp::TopK(_) => "TopK",
        AggOp::BloomMember(_) => "BloomMember",
        AggOp::Entropy(_) => "Entropy",
        AggOp::Windowed(_) => "Windowed",
        AggOp::First(_) => "First",
        AggOp::Last(_) => "Last",
        AggOp::FirstN(_) => "FirstN",
        AggOp::LastN(_) => "LastN",
        AggOp::Lag(_) => "Lag",
        AggOp::FirstSeen(_) => "FirstSeen",
        AggOp::LastSeen(_) => "LastSeen",
        AggOp::Age(_) => "Age",
        AggOp::HasSeen(_) => "HasSeen",
        AggOp::TimeSince(_) => "TimeSince",
        AggOp::TimeSinceLastN(_) => "TimeSinceLastN",
        AggOp::Streak(_) => "Streak",
        AggOp::MaxStreak(_) => "MaxStreak",
        AggOp::NegativeStreak(_) => "NegativeStreak",
        AggOp::FirstSeenInWindow(_) => "FirstSeenInWindow",
        AggOp::Ewma(_) => "Ewma",
        AggOp::EwVar(_) => "EwVar",
        AggOp::EwZScore(_) => "EwZScore",
        AggOp::DecayedSum(_) => "DecayedSum",
        AggOp::DecayedCount(_) => "DecayedCount",
        AggOp::Twa(_) => "Twa",
        AggOp::RateOfChange(_) => "RateOfChange",
        AggOp::InterArrivalStats(_) => "InterArrivalStats",
        AggOp::BurstCount(_) => "BurstCount",
        AggOp::DeltaFromPrev(_) => "DeltaFromPrev",
        AggOp::Trend(_) => "Trend",
        AggOp::TrendResidual(_) => "TrendResidual",
        AggOp::OutlierCount(_) => "OutlierCount",
        AggOp::ValueChangeCount(_) => "ValueChangeCount",
        AggOp::ZScore(_) => "ZScore",
        AggOp::Histogram(_) => "Histogram",
        AggOp::HourOfDayHistogram(_) => "HourOfDayHistogram",
        AggOp::DowHourHistogram(_) => "DowHourHistogram",
        AggOp::SeasonalDeviation(_) => "SeasonalDeviation",
        AggOp::EventTypeMix(_) => "EventTypeMix",
        AggOp::MostRecentN(_) => "MostRecentN",
        AggOp::ReservoirSample(_) => "ReservoirSample",
        AggOp::GeoVelocity(_) => "GeoVelocity",
        AggOp::GeoDistance(_) => "GeoDistance",
        AggOp::GeoSpread(_) => "GeoSpread",
        AggOp::DistanceFromHome(_) => "DistanceFromHome",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agg_buffer::{HourOfDayHistogramState, SeasonalDeviationState};
    use crate::agg_geo::{GeoDistanceState, GeoSpreadState, GeoVelocityState};
    use crate::agg_op::{AggKind, AggOp, AggOpDescriptor};
    use crate::agg_state::{CountDistinctStateWrap, CountState, SumState};
    use crate::agg_state_velocity::TrendResidualState;
    use crate::row::{Row, Value};

    #[test]
    fn mem_usage_total_bytes_adds_stack_and_heap() {
        let mut profile = MemProfile::new("sample", 80);
        profile.add_breakdown("vec", 32, "Vec", "test");
        profile.add_breakdown("box", 16, "Box", "test");
        assert_eq!(profile.total_bytes(), 128);
    }

    #[test]
    fn mem_usage_scalar_aggop_reports_enum_stack_slot() {
        let profile = AggOp::Count(Default::default()).mem_profile();
        assert_eq!(profile.stack_bytes, size_of::<AggOp>());
    }

    #[test]
    fn mem_usage_stack_composition_tracks_payload_and_slack() {
        let profile = MemProfile::new("sample", 80).with_stack_composition(80, 8);
        assert_eq!(profile.stack_bytes, 80);
        assert_eq!(profile.enum_slot_bytes, 80);
        assert_eq!(profile.payload_bytes, 8);
        assert_eq!(profile.slack_bytes, 72);
        assert_eq!(profile.total_bytes(), 80);
    }

    #[test]
    fn mem_usage_aggop_payload_size_uses_active_variant_payload() {
        let count = AggOp::Count(CountState::default()).mem_profile();
        assert_eq!(count.stack_bytes, size_of::<AggOp>());
        assert_eq!(count.enum_slot_bytes, size_of::<AggOp>());
        assert_eq!(count.payload_bytes, size_of::<CountState>());
        assert_eq!(
            count.slack_bytes,
            size_of::<AggOp>() - size_of::<CountState>()
        );

        let sum = AggOp::Sum(SumState::default()).mem_profile();
        assert_eq!(sum.payload_bytes, size_of::<SumState>());
        assert!(sum.slack_bytes > 0);

        let trend_residual = AggOp::TrendResidual(TrendResidualState::default()).mem_profile();
        assert_eq!(
            trend_residual.payload_bytes,
            size_of::<TrendResidualState>()
        );
    }

    #[test]
    fn mem_usage_boxed_aggop_payload_is_pointer_not_pointee() {
        let profile =
            AggOp::CountDistinct(Box::new(CountDistinctStateWrap::default())).mem_profile();
        assert_eq!(profile.stack_bytes, size_of::<AggOp>());
        assert_eq!(
            profile.payload_bytes,
            size_of::<Box<CountDistinctStateWrap>>()
        );
        assert!(profile.slack_bytes > 0);
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "Box<CountDistinctStateWrap>"));
    }

    #[test]
    fn mem_usage_boxed_sketch_reports_box_breakdown() {
        let profile = AggOp::new(&AggOpDescriptor {
            kind: AggKind::Percentile,
            ..Default::default()
        })
        .mem_profile();
        assert!(profile.breakdown.iter().any(|b| b.label.contains("Box")));
    }

    #[test]
    fn mem_usage_fixed_boxed_ops_do_not_use_serialized_proxy() {
        let ops = [
            AggOp::HourOfDayHistogram(Box::<HourOfDayHistogramState>::default()),
            AggOp::SeasonalDeviation(Box::<SeasonalDeviationState>::default()),
            AggOp::GeoVelocity(Box::new(GeoVelocityState::with_fields(
                "lat".into(),
                "lon".into(),
            ))),
            AggOp::GeoDistance(Box::new(GeoDistanceState::with_fields(
                "lat".into(),
                "lon".into(),
            ))),
            AggOp::GeoSpread(Box::new(GeoSpreadState::with_fields(
                "lat".into(),
                "lon".into(),
            ))),
        ];
        for op in ops {
            let profile = op.mem_profile();
            assert!(
                !profile.breakdown.iter().any(|entry| {
                    entry.kind == "estimate" || entry.label.contains("owned internals")
                }),
                "{} should use field-aware exact accounting: {:?}",
                profile.label,
                profile.breakdown
            );
            assert!(
                profile.breakdown.iter().any(|entry| entry.kind == "Box"),
                "{} should still report the boxed payload",
                profile.label
            );
        }
    }

    #[test]
    fn mem_usage_count_distinct_reports_mode_specific_components() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::CountDistinct,
            field: Some("merchant_id".into()),
            ..Default::default()
        });
        for i in 0..32 {
            let row = Row::new().with_field("merchant_id", Value::Str(format!("m{i}").into()));
            op.update(&row, i as i64, Some("merchant_id"), true);
        }
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "CountDistinct hash-set slots"));
    }

    #[test]
    fn mem_usage_percentile_sketch_reports_uddsketch_vectors() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::Percentile,
            field: Some("amount".into()),
            ..Default::default()
        });
        for i in 0..300 {
            let row = Row::new().with_field("amount", Value::F64(i as f64 + 1.0));
            op.update(&row, i as i64, Some("amount"), true);
        }
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "UDDSketch positive buckets"));
    }

    #[test]
    fn mem_usage_top_k_hybrid_reports_cms_and_heap_components() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::TopK,
            field: Some("merchant_id".into()),
            ..Default::default()
        });
        for i in 0..1100 {
            let row = Row::new().with_field("merchant_id", Value::Str(format!("m{i}").into()));
            op.update(&row, i as i64, Some("merchant_id"), true);
        }
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "TopK count-min counters"));
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "TopK heap index map"));
    }

    #[test]
    fn mem_usage_bloom_reports_filter_words() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::BloomMember,
            field: Some("email_domain".into()),
            ..Default::default()
        });
        let row = Row::new().with_field("email_domain", Value::Str("risk.test".into()));
        op.update(&row, 1, Some("email_domain"), true);
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "Bloom filter words"));
    }

    #[test]
    fn mem_usage_entropy_reports_category_map_components() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::Entropy,
            field: Some("mcc".into()),
            ..Default::default()
        });
        for value in ["5411", "5732", "5812"] {
            let row = Row::new().with_field("mcc", Value::Str(value.into()));
            op.update(&row, 1, Some("mcc"), true);
        }
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "Entropy category map entries"));
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "Entropy category string capacity"));
    }

    #[test]
    fn mem_usage_vector_backed_state_reports_capacity_bytes() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::FirstN,
            field: Some("merchant_id".into()),
            n: Some(5),
            ..Default::default()
        });
        let row = Row::new().with_field("merchant_id", Value::Str("m1".into()));
        op.update(&row, 1, Some("merchant_id"), true);
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label == "FirstN values" && b.bytes >= size_of::<Value>()));
    }

    #[test]
    fn mem_usage_windowed_state_reports_nested_bucket() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::Sum,
            field: Some("amount".into()),
            window_ms: Some(60_000),
            ..Default::default()
        });
        let row = Row::new().with_field("amount", Value::F64(42.0));
        op.update(&row, 1_000, Some("amount"), true);
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.label.contains("Windowed bucket 0 Box<AggOp>")));
    }

    #[test]
    fn mem_usage_map_backed_state_labels_estimate() {
        let mut op = AggOp::new(&AggOpDescriptor {
            kind: AggKind::EventTypeMix,
            field: Some("mcc".into()),
            ..Default::default()
        });
        let row = Row::new().with_field("mcc", Value::Str("5411".into()));
        op.update(&row, 1, Some("mcc"), true);
        let profile = op.mem_profile();
        assert!(profile
            .breakdown
            .iter()
            .any(|b| b.note.contains("estimated")));
    }

    #[test]
    fn mem_usage_sort_profiles_desc_orders_by_total_bytes() {
        let mut rows = vec![MemProfile::new("small", 1), MemProfile::new("large", 10)];
        sort_profiles_desc(&mut rows);
        assert_eq!(rows[0].label, "large");
    }

    #[test]
    fn serde_profile_estimate_is_deterministic() {
        let profile = serde_profile("value", &serde_json::json!({"a": 1}));
        assert!(profile.heap_bytes > 0);
    }
}
