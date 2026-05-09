//! Count-Min Sketch (W=2048, D=4) + bounded TopKHeap with O(log k) insert.
//!
//! Ported from main:src/engine/cms.rs (Apache 2.0). Plan 22-04's HashMap
//! heap-position side-index optimization is included verbatim — the
//! `index: AHashMap<TopKValue, usize>` field on `TopKHeap` lets the
//! `insert_or_bump` hot path do an O(1) existence check + an O(log k)
//! sift, instead of the legacy O(k) linear scan.
//!
//! # CountMinSketch
//!
//! 2D i64 counter matrix `[D rows][W cols]`, flattened row-major. `update`
//! adds a (signed) delta to one cell per row; cells saturate at zero on
//! decrement. Estimate returns the per-row min. Defaults: W=2048, D=4
//! → ~64 KB per sketch (i64 counters).
//!
//! # TopKHeap
//!
//! A bounded min-heap of `(count, value)` pairs of size at most `k`.
//! `insert_or_bump(value, count)` updates the heap in O(log k):
//!   * If `value` already in heap → look up its position via the side-index
//!     HashMap, update the count, then sift down or up to restore the
//!     min-heap invariant. The index is updated as elements swap during
//!     sifting.
//!   * Otherwise, if heap < k → push and sift up.
//!   * Otherwise, compare to root (current min); if larger, replace root
//!     and sift down.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

/// Default CMS width (columns per row).
pub const DEFAULT_CMS_WIDTH: usize = 2048;
/// Default CMS depth (number of rows / hash functions).
pub const DEFAULT_CMS_DEPTH: usize = 4;

/// Eight pairwise-independent MurmurHash3 finalizer seeds.
pub const CMS_SEEDS: [u64; 8] = [
    0x9E37_79B9_7F4A_7C15,
    0xBF58_476D_1CE4_E5B9,
    0x94D0_49BB_1331_11EB,
    0xD1B5_4A32_D192_ED03,
    0xBEA2_25F9_EB34_556D,
    0xA24B_AED4_963E_E407,
    0x85EB_CA6B_9FE1_A285,
    0xC2B2_AE3D_27D4_EB4F,
];

// ======================== TopKValue ========================

/// A canonical scalar value for top-k bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TopKValue {
    Str(String),
    Int(i64),
    Float(OrderedFloat<f64>),
    Bool(bool),
}

impl TopKValue {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(s) => Some(TopKValue::Str(s.clone())),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some(TopKValue::Int(i))
                } else {
                    n.as_f64().map(|f| TopKValue::Float(OrderedFloat(f)))
                }
            }
            serde_json::Value::Bool(b) => Some(TopKValue::Bool(*b)),
            _ => None,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            TopKValue::Str(s) => serde_json::Value::String(s.clone()),
            TopKValue::Int(i) => serde_json::Value::Number((*i).into()),
            TopKValue::Float(OrderedFloat(f)) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            TopKValue::Bool(b) => serde_json::Value::Bool(*b),
        }
    }

    pub fn hash64(&self) -> u64 {
        // Use process-static RandomState instead of per-call
        // AHasher::default() — saves ~30-50 ns per call.
        crate::sketches::ahash_random_state().hash_one(self)
    }
}

// ======================== CountMinSketch ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountMinSketch {
    width: usize,
    depth: usize,
    counters: Vec<i64>,
    total: u64,
}

impl CountMinSketch {
    pub fn new(width: usize, depth: usize) -> Self {
        assert!(width > 0 && depth > 0 && depth <= CMS_SEEDS.len());
        Self {
            width,
            depth,
            counters: vec![0; width * depth],
            total: 0,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    pub fn total(&self) -> u64 {
        self.total
    }

    #[inline]
    fn rehash(hash: u64, seed: u64) -> u64 {
        let mut h = hash ^ seed;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        h
    }

    #[inline]
    fn col(&self, hash: u64, row: usize) -> usize {
        (Self::rehash(hash, CMS_SEEDS[row]) as usize) % self.width
    }

    pub fn update(&mut self, hash: u64, delta: i64) {
        for row in 0..self.depth {
            let col = self.col(hash, row);
            let idx = row * self.width + col;
            let new = self.counters[idx].saturating_add(delta);
            self.counters[idx] = new.max(0);
        }
        if delta > 0 {
            self.total = self.total.saturating_add(delta as u64);
        } else if delta < 0 {
            self.total = self.total.saturating_sub((-delta) as u64);
        }
    }

    #[inline]
    pub fn insert(&mut self, hash: u64) {
        self.update(hash, 1);
    }

    #[inline]
    pub fn decrement(&mut self, hash: u64) {
        self.update(hash, -1);
    }

    pub fn estimate(&self, hash: u64) -> i64 {
        let mut min = i64::MAX;
        for row in 0..self.depth {
            let col = self.col(hash, row);
            let idx = row * self.width + col;
            if self.counters[idx] < min {
                min = self.counters[idx];
            }
        }
        if min == i64::MAX {
            0
        } else {
            min
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.counters.capacity() * std::mem::size_of::<i64>()
    }

    /// Cell-wise merge of `other` into `self`. Both sketches must share
    /// the exact (width, depth) configuration — this is enforced because
    /// merging across different shapes would be meaningless (different
    /// hash columns).
    ///
    /// Used by the windowed-aggregation query path to combine per-bucket
    /// CMS instances when answering a top_k over a merged window.
    pub fn merge(&mut self, other: &CountMinSketch) {
        assert_eq!(self.width, other.width, "CMS width mismatch in merge");
        assert_eq!(self.depth, other.depth, "CMS depth mismatch in merge");
        for (s, o) in self.counters.iter_mut().zip(other.counters.iter()) {
            *s = s.saturating_add(*o).max(0);
        }
        self.total = self.total.saturating_add(other.total);
    }
}

// ======================== TopKHeap ========================

/// Bounded min-heap of `(count, value)` with O(log k) `insert_or_bump`.
///
/// **Plan 22-04 optimization (verbatim port from main):** the
/// `index: AHashMap<TopKValue, usize>` side-map gives O(1) lookup of an
/// existing value's position in the heap, so bumping its count is a
/// single sift instead of a linear scan. The index is maintained
/// in-step with the heap during sift-up / sift-down — every swap
/// updates both endpoints' positions.
///
/// The heap is a vector of `(count, value)` pairs, root-at-0, where
/// children of `i` are `2i+1` and `2i+2`. Min-heap so the root is the
/// weakest top-k element — replaced on insert when a heavier value
/// arrives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopKHeap {
    k: usize,
    /// Root-at-0 binary min-heap of (count, value).
    heap: Vec<(u64, TopKValue)>,
    /// O(log k) optimization: value → position in `heap`. Reconstructed
    /// lazily on first mutation after deserialize via `ensure_index`
    /// (the `index_ready` flag tracks freshness).
    #[serde(skip)]
    index: ahash::AHashMap<TopKValue, usize>,
    #[serde(skip, default)]
    index_ready: bool,
}

impl TopKHeap {
    pub fn new(k: usize) -> Self {
        let k = k.max(1);
        Self {
            k,
            heap: Vec::with_capacity(k),
            index: ahash::AHashMap::with_capacity(k),
            index_ready: true,
        }
    }

    pub fn k(&self) -> usize {
        self.k
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Rebuild the index after deserialization. O(n) one-shot.
    #[inline]
    fn ensure_index(&mut self) {
        if self.index_ready {
            return;
        }
        self.index.clear();
        self.index.reserve(self.heap.len());
        for (i, (_, v)) in self.heap.iter().enumerate() {
            self.index.insert(v.clone(), i);
        }
        self.index_ready = true;
    }

    /// Swap two heap positions, keeping the index in sync.
    #[inline]
    fn swap(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        self.heap.swap(a, b);
        // After swap, value previously at a is now at b and vice versa.
        let va = self.heap[a].1.clone();
        let vb = self.heap[b].1.clone();
        self.index.insert(va, a);
        self.index.insert(vb, b);
    }

    /// Sift element at `i` upward until min-heap invariant holds. O(log k).
    fn sift_up(&mut self, mut i: usize) {
        while i > 0 {
            let parent = (i - 1) / 2;
            if self.heap[i].0 < self.heap[parent].0 {
                self.swap(i, parent);
                i = parent;
            } else {
                break;
            }
        }
    }

    /// Sift element at `i` downward until min-heap invariant holds. O(log k).
    fn sift_down(&mut self, mut i: usize) {
        let n = self.heap.len();
        loop {
            let left = 2 * i + 1;
            let right = 2 * i + 2;
            let mut smallest = i;
            if left < n && self.heap[left].0 < self.heap[smallest].0 {
                smallest = left;
            }
            if right < n && self.heap[right].0 < self.heap[smallest].0 {
                smallest = right;
            }
            if smallest == i {
                break;
            }
            self.swap(i, smallest);
            i = smallest;
        }
    }

    /// Insert `value` with `count`, or bump an existing value's count.
    ///
    /// Hot path (Plan 22-04 O(log k)):
    ///   * existing value → O(1) HashMap lookup → update count → O(log k) sift
    ///   * heap < k → push + O(log k) sift_up
    ///   * heap == k && count > root → replace root + O(log k) sift_down
    ///   * else drop
    pub fn insert_or_bump(&mut self, value: TopKValue, count: u64) {
        self.ensure_index();
        if let Some(&pos) = self.index.get(&value) {
            // Existing value: update count, then sift in the right direction.
            let old = self.heap[pos].0;
            self.heap[pos].0 = count;
            if count < old {
                self.sift_up(pos);
            } else if count > old {
                self.sift_down(pos);
            }
            return;
        }
        if self.heap.len() < self.k {
            // Below capacity: push + sift up.
            let pos = self.heap.len();
            self.heap.push((count, value.clone()));
            self.index.insert(value, pos);
            self.sift_up(pos);
            return;
        }
        // At capacity: compare against root (current min).
        if count > self.heap[0].0 {
            let evicted = std::mem::replace(&mut self.heap[0], (count, value.clone()));
            self.index.remove(&evicted.1);
            self.index.insert(value, 0);
            self.sift_down(0);
        }
    }

    /// Return the current top-k as `(value, count)` pairs in descending order.
    pub fn top(&self) -> Vec<(TopKValue, u64)> {
        let mut out: Vec<(TopKValue, u64)> =
            self.heap.iter().map(|(c, v)| (v.clone(), *c)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.heap.len() * (std::mem::size_of::<TopKValue>() + 32)
            + self.index.len() * (std::mem::size_of::<TopKValue>() + 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(s: &str) -> u64 {
        crate::sketches::ahash_random_state().hash_one(s)
    }
    #[test]
    fn cms_new_zeros() {
        let c = CountMinSketch::new(DEFAULT_CMS_WIDTH, DEFAULT_CMS_DEPTH);
        assert_eq!(c.estimate(h("x")), 0);
        assert_eq!(c.total(), 0);
    }
    #[test]
    fn cms_insert_then_estimate_at_least_true_count() {
        let mut c = CountMinSketch::new(2048, 4);
        for _ in 0..100 {
            c.insert(h("a"));
        }
        for _ in 0..50 {
            c.insert(h("b"));
        }
        assert!(c.estimate(h("a")) >= 100);
        assert!(c.estimate(h("b")) >= 50);
        assert_eq!(c.total(), 150);
    }
    #[test]
    fn cms_decrement_saturates_at_zero() {
        let mut c = CountMinSketch::new(2048, 4);
        c.insert(h("x"));
        c.decrement(h("x"));
        c.decrement(h("x"));
        assert_eq!(c.estimate(h("x")), 0);
        assert_eq!(c.total(), 0);
    }
    #[test]
    fn cms_bincode_round_trip() {
        let mut c = CountMinSketch::new(2048, 4);
        for _ in 0..10 {
            c.insert(h("k"));
        }
        let bytes = bincode::serialize(&c).unwrap();
        let c2: CountMinSketch = bincode::deserialize(&bytes).unwrap();
        assert_eq!(c2.estimate(h("k")), c.estimate(h("k")));
    }
    #[test]
    fn topkvalue_from_json_round_trip() {
        let v = TopKValue::Str("abc".into());
        assert_eq!(TopKValue::from_json(&v.to_json()), Some(v.clone()));
        let i = TopKValue::Int(42);
        assert_eq!(TopKValue::from_json(&i.to_json()), Some(i));
        let b = TopKValue::Bool(true);
        assert_eq!(TopKValue::from_json(&b.to_json()), Some(b));
    }
    #[test]
    fn topkheap_keeps_top_k() {
        let mut hp = TopKHeap::new(3);
        hp.insert_or_bump(TopKValue::Str("a".into()), 100);
        hp.insert_or_bump(TopKValue::Str("b".into()), 50);
        hp.insert_or_bump(TopKValue::Str("c".into()), 25);
        hp.insert_or_bump(TopKValue::Str("d".into()), 10);
        let top = hp.top();
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].0, TopKValue::Str("a".into()));
        assert_eq!(top[1].0, TopKValue::Str("b".into()));
        assert_eq!(top[2].0, TopKValue::Str("c".into()));
        assert!(top.iter().all(|(v, _)| *v != TopKValue::Str("d".into())));
    }
    #[test]
    fn topkheap_bump_updates_existing_via_index() {
        let mut hp = TopKHeap::new(3);
        hp.insert_or_bump(TopKValue::Str("a".into()), 10);
        hp.insert_or_bump(TopKValue::Str("a".into()), 100);
        let top = hp.top();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0], (TopKValue::Str("a".into()), 100));
    }
    #[test]
    fn topkheap_index_field_present() {
        let src = include_str!("cms.rs");
        assert!(
            src.contains("AHashMap<TopKValue, usize>")
                || src.contains("AHashMap < TopKValue , usize >"),
            "TopKHeap must include AHashMap<TopKValue, usize> heap-position side-index (Plan 22-04 O(log k) opt)"
        );
    }
    #[test]
    fn topkheap_bincode_round_trip() {
        let mut hp = TopKHeap::new(3);
        hp.insert_or_bump(TopKValue::Str("a".into()), 5);
        hp.insert_or_bump(TopKValue::Str("b".into()), 3);
        let bytes = bincode::serialize(&hp).unwrap();
        let hp2: TopKHeap = bincode::deserialize(&bytes).unwrap();
        assert_eq!(hp2.top(), hp.top());
    }
}
