//! Shannon entropy (bits, log2) over a categorical histogram.
//! Greenfield (no main prior art). Cap-and-spill: distinct categories beyond
//! cap collapse into a single "__beava_other__" bucket so memory stays bounded.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SPILL_KEY: &str = "__beava_other__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntropyHistogram {
    /// Per-category counts. BTreeMap → deterministic iteration order (Phase 5 D-06).
    counts: BTreeMap<String, u64>,
    /// Total observations (sum of counts).
    total: u64,
    /// Max distinct categories before novel keys spill to "__beava_other__".
    cap: usize,
    /// Count of observations that spilled (in addition to being recorded under SPILL_KEY).
    spill: u64,
}

impl EntropyHistogram {
    pub fn new(cap: usize) -> Self {
        Self {
            counts: BTreeMap::new(),
            total: 0,
            cap: cap.max(2),
            spill: 0,
        }
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn distinct_count(&self) -> usize {
        self.counts.len()
    }

    pub fn spill_count(&self) -> u64 {
        self.spill
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Returns `true` if `value` is already a tracked category (i.e. has a
    /// non-spill entry in the counts map).
    pub fn contains_category(&self, value: &str) -> bool {
        self.counts.contains_key(value)
    }

    /// Returns the number of distinct tracked categories (including the spill
    /// bucket `"__beava_other__"` if it exists).
    pub fn category_count(&self) -> usize {
        self.counts.len()
    }

    pub fn key_capacity_bytes(&self) -> usize {
        self.counts.keys().map(|k| k.capacity()).sum()
    }

    pub fn insert(&mut self, value: &str) {
        self.total = self.total.saturating_add(1);
        // 1) Already-tracked key → bump and return.
        if let Some(c) = self.counts.get_mut(value) {
            *c += 1;
            return;
        }
        // 2) Spill bucket already exists, novel key → spill.
        if self.counts.contains_key(SPILL_KEY) {
            self.spill += 1;
            *self.counts.get_mut(SPILL_KEY).unwrap() += 1;
            return;
        }
        // 3) Have headroom → insert as new category.
        if self.counts.len() < self.cap {
            self.counts.insert(value.to_string(), 1);
            return;
        }
        // 4) At cap with no spill bucket yet → create spill bucket.
        self.spill += 1;
        self.counts.insert(SPILL_KEY.to_string(), 1);
    }

    pub fn merge(&mut self, other: &EntropyHistogram) {
        for (k, v) in &other.counts {
            *self.counts.entry(k.clone()).or_insert(0) += v;
        }
        self.total = self.total.saturating_add(other.total);
        self.spill = self.spill.saturating_add(other.spill);
    }

    pub fn entropy_bits(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let n = self.total as f64;
        let mut h = 0.0_f64;
        for &c in self.counts.values() {
            if c == 0 {
                continue;
            }
            let p = c as f64 / n;
            h -= p * p.log2();
        }
        h
    }

    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .counts
                .keys()
                .map(|k| k.capacity() + std::mem::size_of::<u64>())
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_zero() {
        let h = EntropyHistogram::new(1024);
        assert_eq!(h.entropy_bits(), 0.0);
    }

    #[test]
    fn single_category_returns_zero() {
        let mut h = EntropyHistogram::new(1024);
        for _ in 0..10 {
            h.insert("a");
        }
        assert!((h.entropy_bits() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn uniform_two_categories_returns_one_bit() {
        let mut h = EntropyHistogram::new(1024);
        for _ in 0..100 {
            h.insert("a");
            h.insert("b");
        }
        assert!((h.entropy_bits() - 1.0).abs() < 0.01);
    }

    #[test]
    fn uniform_n_categories_returns_log2_n() {
        let mut h = EntropyHistogram::new(1024);
        for k in 0..8 {
            for _ in 0..100 {
                h.insert(&format!("c{}", k));
            }
        }
        // log2(8) = 3.0
        assert!((h.entropy_bits() - 3.0).abs() < 0.01);
    }

    #[test]
    fn cap_and_spill_at_threshold() {
        let mut h = EntropyHistogram::new(4); // tiny cap
        h.insert("a");
        h.insert("b");
        h.insert("c");
        h.insert("d");
        h.insert("e");
        h.insert("f");
        // After cap, e/f spill to __beava_other__
        assert_eq!(h.distinct_count(), 5); // a,b,c,d + __beava_other__
        assert_eq!(h.spill_count(), 2);
    }

    #[test]
    fn merge_combines_histograms() {
        let mut h1 = EntropyHistogram::new(1024);
        h1.insert("a");
        h1.insert("a");
        h1.insert("b");
        let mut h2 = EntropyHistogram::new(1024);
        h2.insert("a");
        h2.insert("c");
        h1.merge(&h2);
        assert_eq!(h1.total(), 5);
        // "a"=3, "b"=1, "c"=1
        let p_a: f64 = 3.0 / 5.0;
        let p_b: f64 = 1.0 / 5.0;
        let p_c: f64 = 1.0 / 5.0;
        let expected = -(p_a * p_a.log2() + p_b * p_b.log2() + p_c * p_c.log2());
        assert!((h1.entropy_bits() - expected).abs() < 1e-9);
    }

    #[test]
    fn bincode_round_trip() {
        let mut h = EntropyHistogram::new(1024);
        h.insert("x");
        h.insert("y");
        h.insert("y");
        let bytes = bincode::serialize(&h).unwrap();
        let h2: EntropyHistogram = bincode::deserialize(&bytes).unwrap();
        assert_eq!(h2.total(), h.total());
        assert!((h2.entropy_bits() - h.entropy_bits()).abs() < 1e-12);
    }
}
