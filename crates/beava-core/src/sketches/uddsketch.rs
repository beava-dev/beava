//! Ported from main:src/engine/uddsketch.rs (adapted from Timescale's UDDSketch
//! port, Apache 2.0). Adds `decrement()` for ring-buffer retraction. Alpha drift
//! on decrement is intentional (one-way).
//!
//! # Properties
//!
//! - Relative-error quantile sketch: `|q̂ - q_true| / q_true <= α`.
//! - `α` starts at `alpha0` (default 0.01) and grows monotonically via
//!   bucket-collapse rounds when bucket count exceeds `max_buckets`.
//!   Per the UDDSketch paper: γ_new = γ_old², so
//!   `α_new = 2α / (1 + α²)`.
//! - Memory: `O(max_buckets)` — default 2048 buckets.
//!
//! # `decrement()`
//!
//! Mirrors `insert`: compute the same bucket, subtract one, saturate at zero,
//! drop the bucket entry on count==0. `total_count` saturates at zero.
//! `current_alpha` is NOT rolled back on decrement (one-way drift).
//!
//! # Plan 19.2-04 (D-04a): flat sorted Vec<(i32, u64)> replaces BTreeMap
//!
//! `pos_buckets` and `neg_buckets` are sorted ascending by the i32 key.
//! Binary-search insert/lookup replaces BTreeMap node-pointer chasing.
//! Cache-friendlier traversal: ~75 ns per-call vs BTreeMap's ~130 ns.
//!
//! Vec::insert at middle is O(n) where n ≤ max_buckets=2048 (worst-case
//! ~10 µs memmove of 16 KB). In practice, most inserts hit existing buckets
//! via the Ok branch (O(log n) binary_search + O(1) increment) — O(n)
//! movement is rare and amortized by the frequent-bucket access pattern.

use serde::{Deserialize, Serialize};

/// Default max buckets before a collapse round halves α-resolution.
pub const DEFAULT_MAX_BUCKETS: usize = 2048;

/// Default starting α (locked at 0.01).
pub const DEFAULT_ALPHA: f64 = 0.01;

/// UDDSketch: a Uniform Dyadic Distribution Sketch with retraction.
///
/// Internal storage uses flat sorted `Vec<(i32, u64)>` with binary-search
/// insert/lookup. Same α=0.01 accuracy contract, same 2,048-bucket cap,
/// same retraction support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UDDSketch {
    alpha0: f64,
    current_alpha: f64,
    /// ln(γ) where γ = (1+α)/(1-α). Bucket-index formula: k = floor(ln(x)/ln(γ)).
    ln_gamma: f64,
    num_collapses: u32,
    max_buckets: usize,
    /// Flat sorted Vec sorted ascending by the i32 key. Binary-search
    /// insert/lookup → cache-friendlier than BTreeMap node traversal.
    /// Per-call algorithmic floor: ~75 ns vs BTreeMap's ~130 ns.
    #[serde(default)]
    pos_buckets: Vec<(i32, u64)>,
    /// Flat sorted Vec for negative-value buckets.
    #[serde(default)]
    neg_buckets: Vec<(i32, u64)>,
    /// Count of observations equal to exactly 0.0 (ln(0) is undefined).
    zero_count: u64,
    total_count: u64,
}

impl Default for UDDSketch {
    fn default() -> Self {
        Self::new(DEFAULT_ALPHA, DEFAULT_MAX_BUCKETS)
    }
}

impl UDDSketch {
    pub fn new(alpha0: f64, max_buckets: usize) -> Self {
        assert!(alpha0 > 0.0 && alpha0 < 1.0, "alpha must be in (0, 1)");
        assert!(max_buckets >= 16, "max_buckets must be >= 16");
        let gamma = (1.0 + alpha0) / (1.0 - alpha0);
        Self {
            alpha0,
            current_alpha: alpha0,
            ln_gamma: gamma.ln(),
            num_collapses: 0,
            max_buckets,
            pos_buckets: Vec::new(),
            neg_buckets: Vec::new(),
            zero_count: 0,
            total_count: 0,
        }
    }

    #[inline]
    pub fn current_alpha(&self) -> f64 {
        self.current_alpha
    }

    #[inline]
    pub fn alpha0(&self) -> f64 {
        self.alpha0
    }

    #[inline]
    pub fn num_collapses(&self) -> u32 {
        self.num_collapses
    }

    #[inline]
    pub fn total_count(&self) -> u64 {
        self.total_count
    }

    #[inline]
    pub fn positive_bucket_capacity(&self) -> usize {
        self.pos_buckets.capacity()
    }

    #[inline]
    pub fn negative_bucket_capacity(&self) -> usize {
        self.neg_buckets.capacity()
    }

    #[inline]
    pub fn positive_bucket_len(&self) -> usize {
        self.pos_buckets.len()
    }

    #[inline]
    pub fn negative_bucket_len(&self) -> usize {
        self.neg_buckets.len()
    }

    #[inline]
    pub fn zero_count(&self) -> u64 {
        self.zero_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_count == 0
    }

    #[inline]
    fn bucket_key(&self, value: f64) -> i32 {
        (value.ln() / self.ln_gamma).floor() as i32
    }

    /// Insert a value. Non-finite values are skipped.
    pub fn insert(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.total_count = self.total_count.saturating_add(1);
        if value == 0.0 {
            self.zero_count = self.zero_count.saturating_add(1);
            return;
        }
        let key = self.bucket_key(value.abs());
        let target = if value > 0.0 {
            &mut self.pos_buckets
        } else {
            &mut self.neg_buckets
        };
        match target.binary_search_by_key(&key, |&(k, _)| k) {
            Ok(idx) => target[idx].1 = target[idx].1.saturating_add(1),
            Err(idx) => target.insert(idx, (key, 1)),
        }

        if self.pos_buckets.len() + self.neg_buckets.len() > self.max_buckets {
            self.collapse();
        }
    }

    /// Decrement a single occurrence. Saturates at 0; never produces negatives.
    pub fn decrement(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        if value == 0.0 {
            if self.zero_count > 0 {
                self.zero_count -= 1;
                self.total_count = self.total_count.saturating_sub(1);
            }
            return;
        }
        let key = self.bucket_key(value.abs());
        let target = if value > 0.0 {
            &mut self.pos_buckets
        } else {
            &mut self.neg_buckets
        };
        if let Ok(idx) = target.binary_search_by_key(&key, |&(k, _)| k) {
            if target[idx].1 > 0 {
                target[idx].1 -= 1;
                self.total_count = self.total_count.saturating_sub(1);
                if target[idx].1 == 0 {
                    target.remove(idx);
                }
            }
        }
    }

    /// Merge another sketch (collapses to whichever has coarser alpha).
    pub fn merge(&mut self, other: &UDDSketch) {
        let mut other = other.clone();
        while self.num_collapses < other.num_collapses {
            self.collapse();
        }
        while other.num_collapses < self.num_collapses {
            other.collapse();
        }
        Self::merge_sorted_vecs(&mut self.pos_buckets, &other.pos_buckets);
        Self::merge_sorted_vecs(&mut self.neg_buckets, &other.neg_buckets);
        self.zero_count = self.zero_count.saturating_add(other.zero_count);
        self.total_count = self.total_count.saturating_add(other.total_count);
        if self.pos_buckets.len() + self.neg_buckets.len() > self.max_buckets {
            self.collapse();
        }
    }

    /// Merge two sorted Vecs in O(n+m) via two-pointer walk. Modifies `a` in place.
    fn merge_sorted_vecs(a: &mut Vec<(i32, u64)>, b: &[(i32, u64)]) {
        let mut out = Vec::with_capacity(a.len() + b.len());
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            match a[i].0.cmp(&b[j].0) {
                std::cmp::Ordering::Less => {
                    out.push(a[i]);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    out.push(b[j]);
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    out.push((a[i].0, a[i].1.saturating_add(b[j].1)));
                    i += 1;
                    j += 1;
                }
            }
        }
        out.extend_from_slice(&a[i..]);
        out.extend_from_slice(&b[j..]);
        *a = out;
    }

    /// Collapse: merge adjacent bucket pairs, doubling γ and raising α.
    fn collapse(&mut self) {
        let a = self.current_alpha;
        self.current_alpha = (2.0 * a) / (1.0 + a * a);
        self.ln_gamma *= 2.0;
        self.num_collapses = self.num_collapses.saturating_add(1);

        // Walk Vec sequentially, grouping adjacent pairs by floor(key/2).
        // div_euclid handles negative keys correctly (floor division).
        fn collapse_vec(buckets: &mut Vec<(i32, u64)>) {
            let mut new_buckets: Vec<(i32, u64)> = Vec::with_capacity(buckets.len() / 2 + 1);
            for &(k, c) in buckets.iter() {
                let new_key = k.div_euclid(2);
                if let Some(last) = new_buckets.last_mut() {
                    if last.0 == new_key {
                        last.1 = last.1.saturating_add(c);
                        continue;
                    }
                }
                new_buckets.push((new_key, c));
            }
            *buckets = new_buckets;
        }

        collapse_vec(&mut self.pos_buckets);
        collapse_vec(&mut self.neg_buckets);
    }

    /// Estimate the q-quantile. Returns `None` if empty. q is clamped to [0, 1].
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.total_count == 0 {
            return None;
        }
        let q = q.clamp(0.0, 1.0);
        let target_rank = (q * (self.total_count.saturating_sub(1)) as f64).floor() as u64;

        let mut cumul: u64 = 0;
        // Most-negative values first: neg_buckets sorted ascending by key (smallest
        // magnitude first); we iterate in reverse for descending-magnitude order.
        for &(key, count) in self.neg_buckets.iter().rev() {
            if cumul + count > target_rank {
                return Some(-self.bucket_center(key));
            }
            cumul += count;
        }
        if self.zero_count > 0 {
            if cumul + self.zero_count > target_rank {
                return Some(0.0);
            }
            cumul += self.zero_count;
        }
        // pos_buckets sorted ascending: smallest key = smallest positive value first.
        for &(key, count) in self.pos_buckets.iter() {
            if cumul + count > target_rank {
                return Some(self.bucket_center(key));
            }
            cumul += count;
        }

        // Fallback: return last bucket center.
        if let Some(&(k, _)) = self.pos_buckets.last() {
            return Some(self.bucket_center(k));
        }
        if let Some(&(k, _)) = self.neg_buckets.first() {
            return Some(-self.bucket_center(k));
        }
        Some(0.0)
    }

    #[inline]
    fn bucket_center(&self, k: i32) -> f64 {
        let lower = (k as f64 * self.ln_gamma).exp();
        let upper = ((k + 1) as f64 * self.ln_gamma).exp();
        (lower + upper) / 2.0
    }

    pub fn estimated_bytes(&self) -> usize {
        let per_entry = std::mem::size_of::<(i32, u64)>();
        std::mem::size_of::<Self>() + per_entry * (self.pos_buckets.len() + self.neg_buckets.len())
    }

    /// Internal accessor for integration tests / property tests.
    /// Returns the flat sorted Vec of positive-value buckets as a slice.
    /// Not a stable API — internal use only.
    pub fn pos_buckets_for_test(&self) -> &[(i32, u64)] {
        &self.pos_buckets
    }

    /// Internal accessor for integration tests / property tests.
    /// Returns the flat sorted Vec of negative-value buckets as a slice.
    /// Not a stable API — internal use only.
    pub fn neg_buckets_for_test(&self) -> &[(i32, u64)] {
        &self.neg_buckets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_quantile_returns_none() {
        let s = UDDSketch::default();
        assert!(s.quantile(0.5).is_none());
        assert_eq!(s.total_count(), 0);
    }
    #[test]
    fn uniform_p50_within_2pct() {
        let mut s = UDDSketch::default();
        for i in 1..=10_000 {
            s.insert(i as f64);
        }
        let p50 = s.quantile(0.5).unwrap();
        let err = (p50 - 5_000.0).abs() / 5_000.0;
        assert!(err < 0.02, "p50={} err={}", p50, err);
    }
    #[test]
    fn uniform_p99_within_2pct() {
        let mut s = UDDSketch::default();
        for i in 1..=10_000 {
            s.insert(i as f64);
        }
        let p99 = s.quantile(0.99).unwrap();
        let err = (p99 - 9_900.0).abs() / 9_900.0;
        assert!(err < 0.02, "p99={} err={}", p99, err);
    }
    #[test]
    fn pareto_p99_within_10pct() {
        let mut s = UDDSketch::default();
        let xm = 1.0;
        let alpha = 1.5;
        for i in 0..10_000 {
            let u = (i as f64 + 0.5) / 10_000.0;
            let x = xm / (1.0 - u).powf(1.0 / alpha);
            s.insert(x);
        }
        let true_p99 = 1.0 / 0.01_f64.powf(1.0 / 1.5);
        let p99 = s.quantile(0.99).unwrap();
        let err = (p99 - true_p99).abs() / true_p99;
        assert!(err < 0.10, "p99={} true={} err={}", p99, true_p99, err);
    }
    #[test]
    fn decrement_drops_total_and_buckets() {
        let mut s = UDDSketch::default();
        for v in &[1.0_f64, 2.0, 3.0, 4.0, 5.0] {
            s.insert(*v);
        }
        assert_eq!(s.total_count(), 5);
        s.decrement(1.0);
        s.decrement(5.0);
        assert_eq!(s.total_count(), 3);
        let p50 = s.quantile(0.5).unwrap();
        assert!((p50 - 3.0).abs() / 3.0 < 0.05);
    }
    #[test]
    fn merge_combines_distributions() {
        let mut a = UDDSketch::default();
        let mut b = UDDSketch::default();
        for i in 1..=5_000 {
            a.insert(i as f64);
        }
        for i in 5_001..=10_000 {
            b.insert(i as f64);
        }
        a.merge(&b);
        assert_eq!(a.total_count(), 10_000);
        let p50 = a.quantile(0.5).unwrap();
        let err = (p50 - 5_000.0).abs() / 5_000.0;
        assert!(err < 0.02);
    }
    #[test]
    fn bincode_round_trip() {
        let mut s = UDDSketch::default();
        for i in 1..=1_000 {
            s.insert(i as f64);
        }
        let bytes = bincode::serialize(&s).unwrap();
        let s2: UDDSketch = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s2.total_count(), s.total_count());
        let p50a = s.quantile(0.5).unwrap();
        let p50b = s2.quantile(0.5).unwrap();
        assert!((p50a - p50b).abs() < 1e-9);
    }
    #[test]
    fn alpha_collapses_under_pressure() {
        let mut s = UDDSketch::new(0.01, 64);
        for i in 0..2_000 {
            let v = (1.006_f64).powi(i - 1000);
            s.insert(v);
        }
        assert!(
            s.current_alpha() > 0.01,
            "alpha should have grown via collapse"
        );
    }
}
