//! Bounded-buffer aggregation operators.
//!
//! AGG-BUFFER-01..07:
//! - HistogramState        — fixed-bucket counts of a numeric field (`buckets[]`)
//! - HourOfDayHistogramState — 24 buckets keyed on event hour
//! - DowHourHistogramState — 168 buckets keyed on (day-of-week, hour)
//! - SeasonalDeviationState — z-score vs hour-of-day baseline
//! - EventTypeMixState     — proportion per category, bounded by `max_categories`
//! - MostRecentNState      — circular buffer of N most-recent values
//! - ReservoirSampleState  — Algorithm R reservoir sample of K values
//!
//! D-06 invariants: no wall-clock reads, no `rand::` (replaced by inline
//! deterministic xorshift seeded from `items_seen`).
//! D-08 (Phase 11 CONTEXT): all operators are lifetime / windowless in v0.

use crate::row::{Row, Value};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

// ─── helpers ─────────────────────────────────────────────────────────────────

fn numeric_from_row(row: &Row, field: &str) -> Option<f64> {
    match row.get(field)? {
        Value::F64(v) => Some(*v),
        Value::I64(v) => Some(*v as f64),
        _ => None,
    }
}

/// Extract a string key from a Row field, borrowing from
/// `Value::Str(CompactString)` without allocating when possible. Allocates only
/// for derived-string types (I64, Bool). Returns `None` for non-string-key types.
///
/// Lifetime `'a` ties the `Cow::Borrowed` variant to the row's borrow scope,
/// which is the apply-event call scope — no lifetime escape.
pub fn str_from_row<'a>(row: &'a Row, field: &str) -> Option<Cow<'a, str>> {
    match row.get(field)? {
        Value::Str(s) => Some(Cow::Borrowed(s.as_str())), // zero alloc
        Value::I64(n) => Some(Cow::Owned(n.to_string())),
        Value::Bool(b) => Some(Cow::Owned(b.to_string())),
        _ => None,
    }
}

// ─── HistogramState (AGG-BUFFER-01) ──────────────────────────────────────────

/// Fixed-bucket count histogram of a numeric field.
///
/// `buckets` is a strictly-increasing list of split points. For
/// `buckets=[10,20,50]` the cells are:
///   `(-inf, 10)`, `[10, 20)`, `[20, 50)`, `[50, +inf)`
/// → `n_cells = buckets.len() + 1`
///
/// Bucket labels follow the convention: `"<10"`, `"10-20"`, `"20-50"`, `">=50"`.
/// Output `Value::Map` is sorted (BTreeMap iteration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramState {
    pub buckets: Vec<f64>,
    pub counts: Vec<u64>,
}

impl HistogramState {
    pub fn new(buckets: Vec<f64>) -> Self {
        let n = buckets.len() + 1;
        Self {
            buckets,
            counts: vec![0; n],
        }
    }

    /// Bucket index for value `v`. Returns `0..buckets.len()+1`.
    fn bucket_index(&self, v: f64) -> usize {
        // Linear scan; for v0 bucket counts (≤ ~20) this is fine.
        for (i, &edge) in self.buckets.iter().enumerate() {
            if v < edge {
                return i;
            }
        }
        self.buckets.len()
    }

    pub fn update(&mut self, row: &Row, field: Option<&str>, where_matched: bool) {
        if !where_matched {
            return;
        }
        let Some(fname) = field else { return };
        let Some(v) = numeric_from_row(row, fname) else {
            return;
        };
        let idx = self.bucket_index(v);
        self.counts[idx] = self.counts[idx].saturating_add(1);
    }

    /// Build label for cell `i`: `"<x"`, `"x-y"`, `">=z"`.
    fn label(&self, i: usize) -> String {
        if self.buckets.is_empty() {
            return "all".to_string();
        }
        if i == 0 {
            return format!("<{}", fmt_edge(self.buckets[0]));
        }
        if i == self.buckets.len() {
            return format!(">={}", fmt_edge(self.buckets[i - 1]));
        }
        format!(
            "{}-{}",
            fmt_edge(self.buckets[i - 1]),
            fmt_edge(self.buckets[i])
        )
    }

    pub fn query(&self) -> Value {
        let mut m = BTreeMap::new();
        for (i, &c) in self.counts.iter().enumerate() {
            m.insert(self.label(i), Value::I64(c as i64));
        }
        Value::Map(m)
    }
}

fn fmt_edge(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

// ─── HourOfDayHistogramState (AGG-BUFFER-02) ─────────────────────────────────

/// 24-bin hour-of-day histogram. Bin index = `(now_ms / 3_600_000) mod 24`.
/// Labels are zero-padded `"00".."23"` (UTC).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HourOfDayHistogramState {
    pub counts: [u64; 24],
}

impl HourOfDayHistogramState {
    pub fn update(&mut self, now_ms: i64, where_matched: bool) {
        if !where_matched {
            return;
        }
        let h = hour_of_day_index(now_ms);
        self.counts[h] = self.counts[h].saturating_add(1);
    }

    pub fn query(&self) -> Value {
        // Emit as Value::List of 24 i64 counts (indexed 0..23
        // by hour-of-day UTC). Was Value::Map with `"00".."23"` string keys
        // which forced callers to do `hist["03"]` lookups; the list shape is
        // simpler (`hist[3]`) and parses to a Python list at the wire boundary.
        let counts: Vec<Value> = self.counts.iter().map(|c| Value::I64(*c as i64)).collect();
        Value::List(counts)
    }
}

/// Hour-of-day index `0..24` (UTC) for an event time in ms-since-epoch.
/// Negative values are normalised by `rem_euclid` so pre-1970 events still
/// map to a valid hour.
pub(crate) fn hour_of_day_index(now_ms: i64) -> usize {
    let hours = now_ms.div_euclid(3_600_000);
    hours.rem_euclid(24) as usize
}

// ─── DowHourHistogramState (AGG-BUFFER-03) ───────────────────────────────────

/// 168-bin (7 day × 24 hour) histogram. Days are Mon..Sun (Unix epoch =
/// Thursday → +4 offset).
/// Labels: `"Mon-00".."Sun-23"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DowHourHistogramState {
    pub counts: Vec<u64>, // 168 entries
}

impl Default for DowHourHistogramState {
    fn default() -> Self {
        Self {
            counts: vec![0; 168],
        }
    }
}

const DAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl DowHourHistogramState {
    pub fn update(&mut self, now_ms: i64, where_matched: bool) {
        if !where_matched {
            return;
        }
        let idx = dow_hour_index(now_ms);
        self.counts[idx] = self.counts[idx].saturating_add(1);
    }

    pub fn query(&self) -> Value {
        let mut m = BTreeMap::new();
        for (d, label) in DAY_LABELS.iter().enumerate() {
            for h in 0..24 {
                let idx = d * 24 + h;
                m.insert(
                    format!("{}-{:02}", label, h),
                    Value::I64(self.counts[idx] as i64),
                );
            }
        }
        Value::Map(m)
    }
}

/// (day-of-week, hour) → flat index `0..168`. Mon=0, Sun=6.
pub(crate) fn dow_hour_index(now_ms: i64) -> usize {
    // Unix epoch (1970-01-01) was a Thursday → day 3 in Mon=0 ordering.
    let days = now_ms.div_euclid(86_400_000);
    let dow = (days + 3).rem_euclid(7) as usize;
    let hour = hour_of_day_index(now_ms);
    dow * 24 + hour
}

// ─── SeasonalDeviationState (AGG-BUFFER-04) ──────────────────────────────────

/// Z-score of the most recent event's field value vs the running hour-of-day
/// baseline (mean + stddev) for that hour.
///
/// Per-hour state: `(count, sum, sum_sq)` — Welford-incompatible but adequate
/// for v0 (single-pass variance via the textbook formula). Returns `Null` if
/// the bucket has fewer than 2 observations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeasonalDeviationState {
    pub per_hour: [HourBucket; 24],
    pub last_observed: Option<(f64, usize)>, // (value, hour_index)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HourBucket {
    pub n: u64,
    pub sum: f64,
    pub sum_sq: f64,
}

impl SeasonalDeviationState {
    pub fn update(&mut self, row: &Row, now_ms: i64, field: Option<&str>, where_matched: bool) {
        if !where_matched {
            return;
        }
        let Some(fname) = field else { return };
        let Some(v) = numeric_from_row(row, fname) else {
            return;
        };
        let h = hour_of_day_index(now_ms);
        let bucket = &mut self.per_hour[h];
        bucket.n += 1;
        bucket.sum += v;
        bucket.sum_sq += v * v;
        self.last_observed = Some((v, h));
    }

    pub fn query(&self) -> Value {
        let Some((v, h)) = self.last_observed else {
            return Value::Null;
        };
        let bucket = &self.per_hour[h];
        if bucket.n < 2 {
            return Value::Null;
        }
        let n = bucket.n as f64;
        let mean = bucket.sum / n;
        // Sample variance via E[X^2] - E[X]^2 with Bessel correction
        let var = (bucket.sum_sq - bucket.sum * bucket.sum / n) / (n - 1.0);
        if var <= 0.0 {
            return Value::Null;
        }
        let stddev = var.sqrt();
        Value::F64((v - mean) / stddev)
    }
}

// ─── EventTypeMixState (AGG-BUFFER-05) ───────────────────────────────────────

/// Proportion of events per category. Bounded by `max_categories`; once full,
/// new categories are silently dropped (their events still increment `total`,
/// matching SQL `OTHER` collapse semantics for v0).
///
/// Categories are pre-declared at register time via the `categories=[...]` kwarg
/// when present; if not specified, we accept any category up to `max_categories`.
///
/// `allowed_set` is the O(1) in-memory companion to
/// `allowed`. Built at `new()` time from `allowed`, and lazily rebuilt on the
/// first update after snapshot deserialization (via `#[serde(skip)]`). The
/// serde-stable field `allowed: Option<Vec<String>>` is kept for snapshot
/// back-compat; production hot-path uses `allowed_set` exclusively.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTypeMixState {
    pub counts: BTreeMap<String, u64>,
    pub total: u64,
    pub max_categories: usize,
    /// Kept as `Vec<String>` for serde stability (snapshot back-compat).
    /// In-memory access uses `allowed_set` for O(1) contains. Built at
    /// `new()` time and rebuilt lazily on first update post-snapshot load.
    pub allowed: Option<Vec<String>>,
    /// O(1) AHashSet companion to `allowed`.
    /// Skipped during serde; rebuilt from `allowed` at `new()` or lazily
    /// on first update after deserialization. Default = None.
    #[serde(skip, default)]
    pub allowed_set: Option<ahash::AHashSet<String>>,
}

impl EventTypeMixState {
    pub fn new(max_categories: usize, allowed: Option<Vec<String>>) -> Self {
        let allowed_set = allowed.as_ref().map(|v| {
            let mut set = ahash::AHashSet::with_capacity(v.len());
            for s in v {
                set.insert(s.clone());
            }
            set
        });
        Self {
            counts: BTreeMap::new(),
            total: 0,
            max_categories,
            allowed,
            allowed_set,
        }
    }

    /// Test accessor for `allowed_set`.
    /// Allows integration tests to verify the AHashSet is built without
    /// accessing the private field directly.
    pub fn allowed_set_for_test(&self) -> Option<&ahash::AHashSet<String>> {
        self.allowed_set.as_ref()
    }

    /// Lazy-init `allowed_set` from `allowed`. Called on the hot path when
    /// `allowed` is Some but `allowed_set` is None (happens after snapshot
    /// deserialization where `#[serde(skip)]` zeros out the set).
    /// Pays once per state per process lifetime after a snapshot load.
    fn ensure_allowed_set(&mut self) {
        if let Some(allow_vec) = &self.allowed {
            if self.allowed_set.is_none() {
                let mut set = ahash::AHashSet::with_capacity(allow_vec.len());
                for s in allow_vec {
                    set.insert(s.clone());
                }
                self.allowed_set = Some(set);
            }
        }
    }

    pub fn update(&mut self, row: &Row, field: Option<&str>, where_matched: bool) {
        if !where_matched {
            return;
        }
        let Some(fname) = field else { return };
        let Some(cat_cow) = str_from_row(row, fname) else {
            return;
        };
        self.total = self.total.saturating_add(1);
        // Lazy-init the AHashSet (covers serde-rehydrated states).
        self.ensure_allowed_set();
        if let Some(allow_set) = &self.allowed_set {
            if !allow_set.contains(cat_cow.as_ref()) {
                // O(1) contains check — rejected event counted in total only.
                return;
            }
        } else if !self.counts.contains_key(cat_cow.as_ref())
            && self.counts.len() >= self.max_categories
        {
            // Cardinality cap reached and this is a new category → drop.
            return;
        }
        // counts uses String keys for serde; allocate only at the accept path.
        let cat_string = cat_cow.into_owned();
        let entry = self.counts.entry(cat_string).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// Apply-path fast-path consuming a pre-extracted
    /// `Option<&Value>` from the apply-loop's `ExtractedFields` array (Plan
    /// 19.2-01). Avoids the `row.get(field)` linear scan that `update()` pays.
    ///
    /// `pre_val` = `None` → no-op (field not present in this event, or the
    /// outer dispatcher resolved no value via the agg-local → union-index
    /// remap). The pre_val is supplied by the apply-loop dispatcher
    /// (`AggOp::update_with_extracted_no_where`); this method must not
    /// re-extract from `extracted[field_idx]` — `field_idx` is agg-local
    /// while `extracted` is union-indexed, and the two only coincide by
    /// accident.
    pub fn update_at(&mut self, pre_val: Option<&Value>, where_matched: bool) {
        if !where_matched {
            return;
        }
        let v = match pre_val {
            Some(v) => v,
            None => return,
        };
        // Convert directly from Value to Cow<str> — no intermediate String.
        let cat_cow: Cow<str> = match v {
            Value::Str(s) => Cow::Borrowed(s.as_str()), // zero alloc
            Value::I64(n) => Cow::Owned(n.to_string()),
            Value::Bool(b) => Cow::Owned(b.to_string()),
            _ => return,
        };
        self.total = self.total.saturating_add(1);
        // Lazy-init the AHashSet (covers serde-rehydrated states).
        self.ensure_allowed_set();
        if let Some(allow_set) = &self.allowed_set {
            if !allow_set.contains(cat_cow.as_ref()) {
                return;
            }
        } else if !self.counts.contains_key(cat_cow.as_ref())
            && self.counts.len() >= self.max_categories
        {
            return;
        }
        let cat_string = cat_cow.into_owned();
        let entry = self.counts.entry(cat_string).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    pub fn query(&self) -> Value {
        let mut out = BTreeMap::new();
        if self.total == 0 {
            return Value::Map(out);
        }
        let denom = self.total as f64;
        for (k, v) in &self.counts {
            out.insert(k.clone(), Value::F64(*v as f64 / denom));
        }
        Value::Map(out)
    }
}

// ─── MostRecentNState (AGG-BUFFER-06) ────────────────────────────────────────

/// Circular buffer of the N most-recent values (any Value type). Output is the
/// buffer in insertion order (oldest → newest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MostRecentNState {
    pub n: usize,
    pub buf: Vec<Value>,
    pub head: usize, // next write position (mod n)
    pub filled: bool,
}

impl MostRecentNState {
    pub fn new(n: usize) -> Self {
        Self {
            n: n.max(1),
            buf: Vec::with_capacity(n.max(1)),
            head: 0,
            filled: false,
        }
    }

    pub fn update(&mut self, row: &Row, field: Option<&str>, where_matched: bool) {
        if !where_matched {
            return;
        }
        let Some(fname) = field else { return };
        let Some(v) = row.get(fname) else { return };
        if matches!(v, Value::Null) {
            return;
        }
        let val = v.clone();
        if !self.filled {
            self.buf.push(val);
            if self.buf.len() == self.n {
                self.filled = true;
                self.head = 0;
            }
        } else {
            self.buf[self.head] = val;
            self.head = (self.head + 1) % self.n;
        }
    }

    pub fn query(&self) -> Value {
        if !self.filled {
            return Value::List(self.buf.clone());
        }
        // Filled: rotate so head is oldest.
        let mut out = Vec::with_capacity(self.n);
        for i in 0..self.n {
            let idx = (self.head + i) % self.n;
            out.push(self.buf[idx].clone());
        }
        Value::List(out)
    }
}

// ─── ReservoirSampleState (AGG-BUFFER-07) ────────────────────────────────────

/// Reservoir sample of K values via Algorithm R (Vitter, 1985).
///
/// Determinism: instead of using `rand`, we drive the random index from a
/// per-state xorshift64 PRNG seeded from the static constant
/// `0x9E37_79B9_7F4A_7C15` (golden-ratio hash) XOR'd with `items_seen`. This
/// keeps replay deterministic — the same event sequence always produces the
/// same reservoir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservoirSampleState {
    pub k: usize,
    pub reservoir: Vec<Value>,
    pub items_seen: u64,
}

impl ReservoirSampleState {
    pub fn new(k: usize) -> Self {
        Self {
            k: k.max(1),
            reservoir: Vec::with_capacity(k.max(1)),
            items_seen: 0,
        }
    }

    /// Deterministic xorshift64 PRNG seeded from a counter.
    fn det_random(&self, salt: u64) -> u64 {
        let mut s = self.items_seen ^ salt ^ 0x9E37_79B9_7F4A_7C15;
        if s == 0 {
            s = 0xD2B7_4E45_72D7_C2B0;
        }
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    }

    pub fn update(&mut self, row: &Row, field: Option<&str>, where_matched: bool) {
        if !where_matched {
            return;
        }
        let Some(fname) = field else { return };
        let Some(v) = row.get(fname) else { return };
        if matches!(v, Value::Null) {
            return;
        }
        let val = v.clone();
        self.items_seen += 1;
        if self.reservoir.len() < self.k {
            self.reservoir.push(val);
            return;
        }
        // Algorithm R: pick j uniformly from 0..items_seen, replace if j < k.
        let r = self.det_random(0xA0B1_C2D3_E4F5_0617);
        let j = (r % self.items_seen) as usize;
        if j < self.k {
            self.reservoir[j] = val;
        }
    }

    pub fn query(&self) -> Value {
        Value::List(self.reservoir.clone())
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn row_with_amount(v: f64) -> Row {
        Row::new().with_field("amount", Value::F64(v))
    }
    fn row_with_str(field: &str, val: &str) -> Row {
        Row::new().with_field(field, Value::Str(val.into()))
    }

    // ── HistogramState ───────────────────────────────────────────────────────

    #[test]
    fn histogram_buckets_partitions_correctly() {
        let mut h = HistogramState::new(vec![10.0, 20.0, 50.0]);
        for v in [5.0, 9.99, 10.0, 15.0, 19.99, 20.0, 35.0, 49.99, 50.0, 100.0] {
            h.update(&row_with_amount(v), Some("amount"), true);
        }
        let q = h.query();
        let m = match q {
            Value::Map(m) => m,
            _ => panic!("expected Map"),
        };
        assert_eq!(m.get("<10"), Some(&Value::I64(2)), "5, 9.99");
        assert_eq!(m.get("10-20"), Some(&Value::I64(3)), "10, 15, 19.99");
        assert_eq!(m.get("20-50"), Some(&Value::I64(3)), "20, 35, 49.99");
        assert_eq!(m.get(">=50"), Some(&Value::I64(2)), "50, 100");
    }

    #[test]
    fn histogram_skips_when_where_matched_false() {
        let mut h = HistogramState::new(vec![10.0]);
        h.update(&row_with_amount(5.0), Some("amount"), false);
        let q = h.query();
        let m = match q {
            Value::Map(m) => m,
            _ => panic!(),
        };
        assert_eq!(m.get("<10"), Some(&Value::I64(0)));
    }

    #[test]
    fn histogram_skips_missing_field() {
        let mut h = HistogramState::new(vec![10.0]);
        h.update(&Row::new(), Some("amount"), true);
        let m = match h.query() {
            Value::Map(m) => m,
            _ => panic!(),
        };
        assert_eq!(m.get("<10"), Some(&Value::I64(0)));
    }

    // ── HourOfDayHistogramState ──────────────────────────────────────────────

    #[test]
    fn hour_of_day_histogram_indexes_correctly() {
        let mut h = HourOfDayHistogramState::default();
        // 1970-01-01 03:00:00 UTC → 3 * 3_600_000 = 10_800_000
        h.update(10_800_000, true);
        h.update(10_800_000, true);
        // 1970-01-01 05:30:00 UTC → 5 * 3_600_000 + 30*60_000
        h.update(5 * 3_600_000 + 30 * 60_000, true);
        // Query returns Value::List(24×I64) indexed
        // 0..23 by hour-of-day UTC.
        let counts = match h.query() {
            Value::List(c) => c,
            other => panic!("expected List, got {:?}", other),
        };
        assert_eq!(counts.len(), 24);
        assert_eq!(counts[3], Value::I64(2));
        assert_eq!(counts[5], Value::I64(1));
        assert_eq!(counts[0], Value::I64(0));
    }

    #[test]
    fn hour_of_day_index_handles_negative() {
        // 1969-12-31 23:00:00 UTC = -3_600_000 ms
        assert_eq!(hour_of_day_index(-3_600_000), 23);
    }

    // ── DowHourHistogramState ────────────────────────────────────────────────

    #[test]
    fn dow_hour_histogram_thursday_epoch() {
        let mut h = DowHourHistogramState::default();
        // Epoch is Thursday 00:00:00 UTC → "Thu-00"
        h.update(0, true);
        let m = match h.query() {
            Value::Map(m) => m,
            _ => panic!(),
        };
        assert_eq!(m.get("Thu-00"), Some(&Value::I64(1)));
        assert_eq!(m.get("Mon-00"), Some(&Value::I64(0)));
    }

    #[test]
    fn dow_hour_histogram_monday_index_zero() {
        // Monday 1970-01-05 00:00:00 UTC = 4 days after epoch
        let monday_ms = 4 * 86_400_000;
        let mut h = DowHourHistogramState::default();
        h.update(monday_ms, true);
        let m = match h.query() {
            Value::Map(m) => m,
            _ => panic!(),
        };
        assert_eq!(m.get("Mon-00"), Some(&Value::I64(1)));
    }

    // ── SeasonalDeviationState ───────────────────────────────────────────────

    #[test]
    fn seasonal_deviation_zscore_against_hour_baseline() {
        let mut s = SeasonalDeviationState::default();
        // Hour=3: feed values 100, 100, 100 → bucket mean=100, var=0 → null
        for _ in 0..3 {
            s.update(&row_with_amount(100.0), 10_800_000, Some("amount"), true);
        }
        // Now feed a 150 at hour 3 — baseline (n=3 prior to update? actually 4 now)
        s.update(&row_with_amount(150.0), 10_800_000, Some("amount"), true);
        // bucket: n=4, sum=450, sum_sq=10000+10000+10000+22500=52500
        // mean = 112.5, var = (52500 - 450^2/4)/3 = (52500-50625)/3 = 625
        // stddev = 25, z = (150 - 112.5)/25 = 1.5
        match s.query() {
            Value::F64(v) => assert!((v - 1.5).abs() < 1e-9, "expected z=1.5, got {v}"),
            other => panic!("expected F64, got {:?}", other),
        }
    }

    #[test]
    fn seasonal_deviation_returns_null_with_one_observation() {
        let mut s = SeasonalDeviationState::default();
        s.update(&row_with_amount(100.0), 0, Some("amount"), true);
        assert_eq!(s.query(), Value::Null);
    }

    // ── EventTypeMixState ────────────────────────────────────────────────────

    #[test]
    fn event_type_mix_returns_proportions() {
        let mut e = EventTypeMixState::new(10, None);
        for _ in 0..3 {
            e.update(&row_with_str("type", "click"), Some("type"), true);
        }
        for _ in 0..1 {
            e.update(&row_with_str("type", "view"), Some("type"), true);
        }
        let m = match e.query() {
            Value::Map(m) => m,
            _ => panic!(),
        };
        if let Value::F64(v) = m.get("click").unwrap() {
            assert!((v - 0.75).abs() < 1e-9);
        } else {
            panic!()
        }
        if let Value::F64(v) = m.get("view").unwrap() {
            assert!((v - 0.25).abs() < 1e-9);
        } else {
            panic!()
        }
    }

    #[test]
    fn event_type_mix_caps_categories() {
        let mut e = EventTypeMixState::new(2, None);
        e.update(&row_with_str("type", "a"), Some("type"), true);
        e.update(&row_with_str("type", "b"), Some("type"), true);
        e.update(&row_with_str("type", "c"), Some("type"), true); // dropped from counts
        e.update(&row_with_str("type", "c"), Some("type"), true); // dropped
        let m = match e.query() {
            Value::Map(m) => m,
            _ => panic!(),
        };
        // total=4, only a + b in the map; c contributes to total only
        assert!(m.contains_key("a"));
        assert!(m.contains_key("b"));
        assert!(!m.contains_key("c"));
    }

    // ── MostRecentNState ─────────────────────────────────────────────────────

    #[test]
    fn most_recent_n_circular_overwrite() {
        let mut s = MostRecentNState::new(3);
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            s.update(&row_with_amount(v), Some("amount"), true);
        }
        let l = match s.query() {
            Value::List(l) => l,
            _ => panic!(),
        };
        assert_eq!(
            l,
            vec![Value::F64(3.0), Value::F64(4.0), Value::F64(5.0)],
            "oldest→newest, last 3"
        );
    }

    #[test]
    fn most_recent_n_partially_filled() {
        let mut s = MostRecentNState::new(5);
        s.update(&row_with_amount(7.0), Some("amount"), true);
        s.update(&row_with_amount(8.0), Some("amount"), true);
        let l = match s.query() {
            Value::List(l) => l,
            _ => panic!(),
        };
        assert_eq!(l, vec![Value::F64(7.0), Value::F64(8.0)]);
    }

    // ── ReservoirSampleState ─────────────────────────────────────────────────

    #[test]
    fn reservoir_sample_fills_to_k() {
        let mut s = ReservoirSampleState::new(3);
        for v in [1.0, 2.0, 3.0] {
            s.update(&row_with_amount(v), Some("amount"), true);
        }
        let l = match s.query() {
            Value::List(l) => l,
            _ => panic!(),
        };
        assert_eq!(l.len(), 3);
    }

    #[test]
    fn reservoir_sample_deterministic_replay() {
        // Same sequence → same reservoir on rerun (D-06 determinism).
        let run = || {
            let mut s = ReservoirSampleState::new(5);
            for v in 0..100 {
                s.update(&row_with_amount(v as f64), Some("amount"), true);
            }
            s.query()
        };
        assert_eq!(run(), run(), "reservoir must be deterministic across runs");
    }

    #[test]
    fn reservoir_sample_keeps_size_at_k() {
        let mut s = ReservoirSampleState::new(5);
        for v in 0..1000 {
            s.update(&row_with_amount(v as f64), Some("amount"), true);
        }
        let l = match s.query() {
            Value::List(l) => l,
            _ => panic!(),
        };
        assert_eq!(l.len(), 5);
    }
}
