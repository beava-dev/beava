//! CountDistinctState — 3-mode hybrid: ExactArray (≤16) → HashSet (≤1024) → HLL p=12.
//! Mode tag uses serde rename for snapshot stability across v0.x.y.
//!
//! Per CONTEXT D-01 (port from main hybrid-distinct), D-04 (serde rename tags),
//! D-05 (memory bounds: ~128B/exact, ~8KB/hashset cap, ~4KB/hll dense).

use crate::sketches::hll::Hll;
use serde::{Deserialize, Serialize};

const EXACT_THRESHOLD: usize = 16;
const HASH_THRESHOLD: usize = 1024;

// Identity hasher for the already-FxHashed u64 input. The HashSet's u64
// keys are FxHasher outputs (see agg_state::hash_value_for_hll); re-hashing
// them with SipHash burned ~1,180 ns/event of apply CPU pre-fix. The
// identity hasher stores the input u64 verbatim as the slot index, with
// hashbrown's SIMD probing handling any clustering. The byte-slice arm is
// unreachable because the only consumer is `HashSet<u64>` whose Hash impl
// calls `Hasher::write_u64`.
#[derive(Default)]
pub(super) struct NoOpHasher {
    state: u64,
}

impl std::hash::Hasher for NoOpHasher {
    #[inline]
    fn write_u64(&mut self, x: u64) {
        self.state = x;
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.state
    }
    fn write(&mut self, _bytes: &[u8]) {
        unreachable!(
            "NoOpHasher::write(&[u8]) is unreachable — CountDistinctState::HashSet \
             is u64-keyed and Hash for u64 calls write_u64 only"
        );
    }
}

type HashSetU64 = hashbrown::HashSet<u64, std::hash::BuildHasherDefault<NoOpHasher>>;

/// Three-mode adaptive distinct-count state. Promotes from `ExactArray` →
/// `HashSet` → `Hll` automatically on insert. Serde tags are stable v0
/// snapshot identifiers (`v0_count_distinct_*`).
// External tagging (default) is required: bincode does not support internally-
// tagged enums (those use `deserialize_any` which bincode lacks). External
// tags still satisfy v0 snapshot stability — the variant rename strings are
// the tag strings emitted in JSON / consumed by bincode's variant index.
//
// The `HashSet` variant's `HashSetU64` alias references the module-private
// `NoOpHasher`. The hasher type stays module-internal (no API surface
// change), but rust's `private_interfaces` lint warns because `NoOpHasher`
// is reachable via the public variant's field type. External callers can
// still construct the variant via `CountDistinctState::new(...)` +
// `add_hash(...)` (the only supported APIs); they cannot name `NoOpHasher`
// or `HashSetU64` directly.
// reason: NoOpHasher is intentionally module-private but reachable through
// the public HashSet variant's HashSetU64 type alias; see the long comment
// above for the API-surface rationale.
#[allow(private_interfaces)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CountDistinctState {
    #[serde(rename = "v0_count_distinct_exact_array")]
    ExactArray { values: Vec<u64> },
    #[serde(rename = "v0_count_distinct_hash_set")]
    HashSet { hashes: HashSetU64 },
    #[serde(rename = "v0_count_distinct_hll")]
    Hll { sketch: Hll },
}

impl CountDistinctState {
    /// Construct an empty state in `ExactArray` mode.
    /// `_hash_threshold` parameter is reserved for v0.1+ configurability;
    /// v0 uses the fixed `HASH_THRESHOLD = 1024` per locked spec (D-01).
    pub fn new(_hash_threshold: usize) -> Self {
        CountDistinctState::ExactArray {
            values: Vec::with_capacity(EXACT_THRESHOLD),
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            CountDistinctState::ExactArray { .. } => "v0_count_distinct_exact_array",
            CountDistinctState::HashSet { .. } => "v0_count_distinct_hash_set",
            CountDistinctState::Hll { .. } => "v0_count_distinct_hll",
        }
    }

    /// Insert a precomputed u64 hash. Promotes mode if threshold exceeded.
    ///
    /// The input u64 is expected to come from a FxHasher-backed hasher;
    /// HLL's internal `mix64` (`Hll::add_hash`) post-processes for
    /// distribution. See `agg_state::hash_value_for_hll`.
    pub fn add_hash(&mut self, hash: u64) {
        match self {
            CountDistinctState::ExactArray { values } => {
                if let Err(pos) = values.binary_search(&hash) {
                    values.insert(pos, hash);
                    if values.len() > EXACT_THRESHOLD {
                        // Promote to HashSet, preserving every value seen.
                        // HashSetU64 uses NoOpHasher so the already-FxHashed
                        // u64 is stored as the slot index without a redundant
                        // SipHash second-hash.
                        let mut set = HashSetU64::with_capacity_and_hasher(
                            HASH_THRESHOLD,
                            std::hash::BuildHasherDefault::<NoOpHasher>::default(),
                        );
                        for &h in values.iter() {
                            set.insert(h);
                        }
                        *self = CountDistinctState::HashSet { hashes: set };
                    }
                }
            }
            CountDistinctState::HashSet { hashes } => {
                hashes.insert(hash);
                if hashes.len() > HASH_THRESHOLD {
                    // Promote to Hll. Re-feed every retained hash so cardinality
                    // estimate remains continuous across the boundary.
                    let mut hll = Hll::new();
                    for &h in hashes.iter() {
                        hll.add_hash(h);
                    }
                    *self = CountDistinctState::Hll { sketch: hll };
                }
            }
            CountDistinctState::Hll { sketch } => {
                sketch.add_hash(hash);
            }
        }
    }

    /// Estimated cardinality.
    pub fn estimate(&self) -> u64 {
        match self {
            CountDistinctState::ExactArray { values } => values.len() as u64,
            CountDistinctState::HashSet { hashes } => hashes.len() as u64,
            CountDistinctState::Hll { sketch } => sketch.estimate(),
        }
    }

    /// Approximate memory footprint in bytes.
    pub fn estimated_bytes(&self) -> usize {
        match self {
            CountDistinctState::ExactArray { values } => {
                std::mem::size_of::<Self>() + values.capacity() * std::mem::size_of::<u64>()
            }
            CountDistinctState::HashSet { hashes } => {
                std::mem::size_of::<Self>() + hashes.capacity() * 16
            }
            CountDistinctState::Hll { sketch } => sketch.estimated_bytes(),
        }
    }

    /// Promote `self` to `Hll` mode in place. No-op if already Hll.
    fn promote_to_hll(&mut self) {
        match self {
            CountDistinctState::Hll { .. } => {}
            CountDistinctState::ExactArray { values } => {
                let mut hll = Hll::new();
                for &h in values.iter() {
                    hll.add_hash(h);
                }
                *self = CountDistinctState::Hll { sketch: hll };
            }
            CountDistinctState::HashSet { hashes } => {
                let mut hll = Hll::new();
                for &h in hashes.iter() {
                    hll.add_hash(h);
                }
                *self = CountDistinctState::Hll { sketch: hll };
            }
        }
    }

    /// Merge `other` into `self` so `self.estimate()` reflects the
    /// distinct-count of the union. Used by the windowed-aggregation
    /// query path so a windowed `count_distinct` aggregates across all
    /// active buckets instead of returning the latest one's estimate.
    pub fn merge(&mut self, other: &CountDistinctState) {
        // If either side is HLL, promote both to HLL and use Hll::merge —
        // it's the only mode that doesn't expose per-element hashes
        // (the sketch is lossy by design).
        if matches!(other, CountDistinctState::Hll { .. }) {
            self.promote_to_hll();
        }
        match (&mut *self, other) {
            (CountDistinctState::Hll { sketch: s }, CountDistinctState::Hll { sketch: o }) => {
                s.merge(o)
            }
            (CountDistinctState::Hll { sketch: s }, CountDistinctState::ExactArray { values }) => {
                for &h in values.iter() {
                    s.add_hash(h);
                }
            }
            (CountDistinctState::Hll { sketch: s }, CountDistinctState::HashSet { hashes }) => {
                for &h in hashes.iter() {
                    s.add_hash(h);
                }
            }
            // Self is non-HLL and other is non-HLL: feed other's hashes
            // through `self.add_hash` so promotion thresholds fire as
            // they would for inserts. `add_hash` mutably borrows self,
            // and we already non-mutably borrowed other for the match;
            // collect first to release that borrow.
            (_, CountDistinctState::ExactArray { values }) => {
                let collected: Vec<u64> = values.clone();
                for h in collected {
                    self.add_hash(h);
                }
            }
            (_, CountDistinctState::HashSet { hashes }) => {
                let collected: Vec<u64> = hashes.iter().copied().collect();
                for h in collected {
                    self.add_hash(h);
                }
            }
            // (_, Hll) handled by the promote-then-fall-through above.
            (_, CountDistinctState::Hll { .. }) => {
                unreachable!("promote_to_hll above ensures self is Hll when other is Hll")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hash_str(s: &str) -> u64 {
        // Deterministic seed — see hll.rs tests for rationale.
        let rs = ahash::RandomState::with_seeds(
            0x243f_6a88_85a3_08d3,
            0x1319_8a2e_0370_7344,
            0xa409_3822_299f_31d0,
            0x082e_fa98_ec4e_6c89,
        );
        rs.hash_one(s)
    }
    #[test]
    fn starts_in_exact_array_mode() {
        let s = CountDistinctState::new(1024);
        assert_eq!(s.mode_name(), "v0_count_distinct_exact_array");
        assert_eq!(s.estimate(), 0);
    }
    #[test]
    fn promotes_array_to_hash_set_at_16() {
        let mut s = CountDistinctState::new(1024);
        for i in 0..20 {
            s.add_hash(hash_str(&format!("k{}", i)));
        }
        assert_eq!(s.mode_name(), "v0_count_distinct_hash_set");
        assert_eq!(s.estimate(), 20);
    }
    #[test]
    fn promotes_hash_set_to_hll_at_threshold() {
        let mut s = CountDistinctState::new(1024);
        for i in 0..1100 {
            s.add_hash(hash_str(&format!("k{}", i)));
        }
        assert_eq!(s.mode_name(), "v0_count_distinct_hll");
        let est = s.estimate();
        let err = (est as i64 - 1100).abs() as f64 / 1100.0;
        assert!(err < 0.05, "promote err {}", err);
    }
    #[test]
    fn promotion_preserves_distinct_count() {
        let mut s = CountDistinctState::new(1024);
        for i in 0..15 {
            s.add_hash(hash_str(&format!("k{}", i)));
        }
        let before = s.estimate();
        s.add_hash(hash_str("k15"));
        s.add_hash(hash_str("k16"));
        assert_eq!(s.mode_name(), "v0_count_distinct_hash_set");
        assert!(s.estimate() >= before + 2);
    }
    #[test]
    fn duplicate_inserts_dont_inflate_count() {
        let mut s = CountDistinctState::new(1024);
        let h = hash_str("dup");
        for _ in 0..10 {
            s.add_hash(h);
        }
        assert_eq!(s.estimate(), 1);
    }
    #[test]
    fn bincode_round_trip_each_mode() {
        // exact_array
        let mut s1 = CountDistinctState::new(1024);
        for i in 0..5 {
            s1.add_hash(hash_str(&format!("k{}", i)));
        }
        let bytes = bincode::serialize(&s1).unwrap();
        let s1r: CountDistinctState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s1r.estimate(), s1.estimate());
        assert_eq!(s1r.mode_name(), "v0_count_distinct_exact_array");
        // hash_set
        let mut s2 = CountDistinctState::new(1024);
        for i in 0..50 {
            s2.add_hash(hash_str(&format!("k{}", i)));
        }
        let bytes = bincode::serialize(&s2).unwrap();
        let s2r: CountDistinctState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s2r.estimate(), s2.estimate());
        assert_eq!(s2r.mode_name(), "v0_count_distinct_hash_set");
        // hll
        let mut s3 = CountDistinctState::new(1024);
        for i in 0..2000 {
            s3.add_hash(hash_str(&format!("k{}", i)));
        }
        let bytes = bincode::serialize(&s3).unwrap();
        let s3r: CountDistinctState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s3r.estimate(), s3.estimate());
        assert_eq!(s3r.mode_name(), "v0_count_distinct_hll");
    }
    #[test]
    fn serde_tag_in_json() {
        let mut s = CountDistinctState::new(1024);
        s.add_hash(hash_str("a"));
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("v0_count_distinct_exact_array"), "json={}", j);
    }

    // NoOpHasher contract.
    use std::hash::Hasher as _StdHasher;

    #[test]
    fn no_op_hasher_returns_input_unchanged() {
        // Independent contract: write_u64(x) → finish() returns x verbatim.
        let mut h = super::NoOpHasher::default();
        h.write_u64(0xDEADBEEFCAFEBABE_u64);
        assert_eq!(h.finish(), 0xDEADBEEFCAFEBABE_u64);
    }

    #[test]
    fn no_op_hasher_panics_on_byte_write() {
        // The byte-slice arm must be unreachable for u64-keyed sets.
        let result = std::panic::catch_unwind(|| {
            let mut h = super::NoOpHasher::default();
            <super::NoOpHasher as _StdHasher>::write(&mut h, &[0u8, 1u8, 2u8, 3u8]);
            h.finish()
        });
        assert!(
            result.is_err(),
            "NoOpHasher::write(&[u8]) must panic; got Ok({:?})",
            result
        );
    }

    #[test]
    fn hashset_mode_handles_sequential_u64_inputs() {
        // Identity hashing on sequential u64s is the worst-case probe pattern;
        // hashbrown's SIMD probe must still resolve correctly.
        let mut s = CountDistinctState::new(1024);
        for i in 0u64..2048u64 {
            s.add_hash(i);
        }
        // 2048 inserts > HASH_THRESHOLD (1024) → final mode is HLL.
        assert_eq!(s.mode_name(), "v0_count_distinct_hll");
        let est = s.estimate();
        let err = (est as i64 - 2048).abs() as f64 / 2048.0;
        assert!(
            err < 0.05,
            "promote err {} (est={}, expected ~2048)",
            err,
            est
        );
    }

    // ── merge ──────────────────────────────────────────────────────────
    //
    // Coverage gap: pre-fix the windowed query for count_distinct only
    // read the latest active bucket (existing comment at the broken arm
    // even called this out as "v0 simplification — future work"). These
    // tests pin the new merge contract for all 9 mode-pair combinations
    // (3 self-modes × 3 other-modes); the cross-mode cases collapse into
    // the same code paths so we only exercise the representatives.

    #[test]
    fn merge_array_plus_array_unions() {
        let mut a = CountDistinctState::new(1024);
        let mut b = CountDistinctState::new(1024);
        for i in 0..5 {
            a.add_hash(hash_str(&format!("a{i}")));
        }
        for i in 0..5 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        a.merge(&b);
        assert_eq!(a.estimate(), 10, "10 distinct values across the union");
    }

    #[test]
    fn merge_array_plus_array_deduplicates_overlap() {
        let mut a = CountDistinctState::new(1024);
        let mut b = CountDistinctState::new(1024);
        for i in 0..5 {
            let h = hash_str(&format!("shared{i}"));
            a.add_hash(h);
            b.add_hash(h);
        }
        a.merge(&b);
        assert_eq!(a.estimate(), 5, "overlapping hashes must dedupe");
    }

    #[test]
    fn merge_array_plus_array_promotes_to_hashset_with_count_preserved() {
        // The small cutover: ExactArray (cap 16) → HashSet during
        // merge. a has 10 distinct; b has 10 distinct; combined 20 > 16
        // must promote during the merge AND preserve every distinct
        // value (HashSet mode reports exact count, so a buggy promotion
        // that dropped values would surface immediately as a mismatched
        // estimate).
        let mut a = CountDistinctState::new(1024);
        let mut b = CountDistinctState::new(1024);
        for i in 0..10 {
            a.add_hash(hash_str(&format!("a{i}")));
        }
        for i in 0..10 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        // Both still ExactArray pre-merge (10 ≤ EXACT_THRESHOLD=16 each).
        assert_eq!(a.mode_name(), "v0_count_distinct_exact_array");
        assert_eq!(b.mode_name(), "v0_count_distinct_exact_array");

        a.merge(&b);

        assert_eq!(a.mode_name(), "v0_count_distinct_hash_set");
        assert_eq!(
            a.estimate(),
            20,
            "all 20 distinct values must survive the ExactArray→HashSet cutover"
        );
    }

    #[test]
    fn merge_promotes_through_thresholds_naturally_to_hll() {
        // The bigger cutover: HashSet → HLL during merge. Both sides
        // are already in HashSet mode pre-merge; their merge crosses
        // HASH_THRESHOLD=1024 and the cutover must preserve approximate
        // cardinality (within HLL's 5% tolerance).
        let mut a = CountDistinctState::new(1024);
        let mut b = CountDistinctState::new(1024);
        for i in 0..600 {
            a.add_hash(hash_str(&format!("a{i}")));
        }
        for i in 0..600 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        assert_eq!(a.mode_name(), "v0_count_distinct_hash_set");
        assert_eq!(b.mode_name(), "v0_count_distinct_hash_set");

        a.merge(&b);

        // Combined 1200 > HASH_THRESHOLD=1024 → promotes to HLL.
        assert_eq!(a.mode_name(), "v0_count_distinct_hll");
        let err = (a.estimate() as i64 - 1200).abs() as f64 / 1200.0;
        assert!(
            err < 0.05,
            "merged HLL estimate must reflect 1200-union after cutover; err {err}"
        );
    }

    #[test]
    fn merge_hll_plus_hll_uses_hll_merge() {
        let mut a = CountDistinctState::new(1024);
        let mut b = CountDistinctState::new(1024);
        for i in 0..2000 {
            a.add_hash(hash_str(&format!("a{i}")));
        }
        for i in 0..2000 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        assert_eq!(a.mode_name(), "v0_count_distinct_hll");
        assert_eq!(b.mode_name(), "v0_count_distinct_hll");
        a.merge(&b);
        assert_eq!(a.mode_name(), "v0_count_distinct_hll");
        let err = (a.estimate() as i64 - 4000).abs() as f64 / 4000.0;
        assert!(err < 0.05, "merged HLL estimate err {err}");
    }

    #[test]
    fn merge_hll_plus_array_promotes_other_through_hll_path() {
        // Self is HLL; other is ExactArray. The (Hll, ExactArray) arm
        // walks other's array and feeds each hash into self's Hll.
        let mut a = CountDistinctState::new(1024);
        for i in 0..2000 {
            a.add_hash(hash_str(&format!("a{i}")));
        }
        assert_eq!(a.mode_name(), "v0_count_distinct_hll");
        let mut b = CountDistinctState::new(1024);
        for i in 0..5 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        assert_eq!(b.mode_name(), "v0_count_distinct_exact_array");
        a.merge(&b);
        assert_eq!(a.mode_name(), "v0_count_distinct_hll");
        let err = (a.estimate() as i64 - 2005).abs() as f64 / 2005.0;
        assert!(err < 0.05, "merged HLL estimate err {err}");
    }

    #[test]
    fn merge_array_plus_hll_promotes_self() {
        // Self is ExactArray; other is HLL. Self gets promoted to HLL
        // first, then merged via Hll::merge.
        let mut a = CountDistinctState::new(1024);
        for i in 0..5 {
            a.add_hash(hash_str(&format!("a{i}")));
        }
        let mut b = CountDistinctState::new(1024);
        for i in 0..2000 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        assert_eq!(a.mode_name(), "v0_count_distinct_exact_array");
        assert_eq!(b.mode_name(), "v0_count_distinct_hll");
        a.merge(&b);
        assert_eq!(a.mode_name(), "v0_count_distinct_hll");
        let err = (a.estimate() as i64 - 2005).abs() as f64 / 2005.0;
        assert!(err < 0.05, "merged HLL estimate err {err}");
    }

    #[test]
    fn merge_into_empty_self_yields_other() {
        let mut a = CountDistinctState::new(1024);
        let mut b = CountDistinctState::new(1024);
        for i in 0..5 {
            b.add_hash(hash_str(&format!("b{i}")));
        }
        a.merge(&b);
        assert_eq!(a.estimate(), 5);
    }
}
