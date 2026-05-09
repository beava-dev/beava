//! TopKState: 2-mode hybrid (BTreeMap exact ≤1024 distinct → CMS+TopKHeap).
//!
//! Mode 1 (Exact): tracks every distinct value → count in a BTreeMap. Exact
//! answers, deterministic ordering. Promoted to Hybrid when distinct count
//! exceeds the configured threshold (default 1024, matching the
//! CountDistinct/Percentile pattern).
//!
//! Mode 2 (Hybrid): CountMinSketch for frequency estimation + a bounded
//! TopKHeap (size k) of candidate top-k values. Promotion folds the
//! BTreeMap counts into the CMS via a single `update(hash, count)` per
//! key, then seeds the heap with the existing counts.

use crate::sketches::cms::{CountMinSketch, TopKHeap, TopKValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TopKState {
    #[serde(rename = "v0_top_k_exact")]
    Exact {
        counts: BTreeMap<TopKValue, u64>,
        k: usize,
        threshold: usize,
        hybrid_width: usize,
        hybrid_depth: usize,
    },
    #[serde(rename = "v0_top_k_hybrid")]
    Hybrid {
        cms: CountMinSketch,
        heap: TopKHeap,
        k: usize,
    },
}

impl TopKState {
    /// Construct a fresh Exact-mode state. `k` is the desired top-k size,
    /// `exact_threshold` is the distinct-value cap before promoting to
    /// Hybrid mode, `hybrid_width`/`hybrid_depth` size the post-promotion
    /// CMS (defaults: 2048 / 4).
    pub fn new(k: usize, exact_threshold: usize, hybrid_width: usize, hybrid_depth: usize) -> Self {
        TopKState::Exact {
            counts: BTreeMap::new(),
            k: k.max(1),
            threshold: exact_threshold.max(2),
            hybrid_width,
            hybrid_depth,
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            TopKState::Exact { .. } => "v0_top_k_exact",
            TopKState::Hybrid { .. } => "v0_top_k_hybrid",
        }
    }

    /// Increment the count for `value` by 1. Promotes Exact → Hybrid when
    /// the distinct-value cap is exceeded.
    pub fn insert(&mut self, value: TopKValue) {
        let need_promote = match self {
            TopKState::Exact {
                counts, threshold, ..
            } => {
                *counts.entry(value).or_insert(0) += 1;
                counts.len() > *threshold
            }
            TopKState::Hybrid { cms, heap, .. } => {
                let h = value.hash64();
                cms.insert(h);
                let est = cms.estimate(h).max(0) as u64;
                heap.insert_or_bump(value, est);
                false
            }
        };
        if need_promote {
            // Take ownership of the Exact contents to rebuild as Hybrid.
            let TopKState::Exact {
                counts,
                k,
                hybrid_width,
                hybrid_depth,
                ..
            } = std::mem::replace(
                self,
                TopKState::Exact {
                    counts: BTreeMap::new(),
                    k: 1,
                    threshold: 2,
                    hybrid_width: 2048,
                    hybrid_depth: 4,
                },
            )
            else {
                unreachable!()
            };
            let mut cms = CountMinSketch::new(hybrid_width, hybrid_depth);
            let mut heap = TopKHeap::new(k);
            // First pass: fold all counts into the CMS.
            for (val, count) in counts.iter() {
                cms.update(val.hash64(), *count as i64);
            }
            // Second pass: seed the heap with current CMS estimates.
            for (val, _) in counts.iter() {
                let est = cms.estimate(val.hash64()).max(0) as u64;
                heap.insert_or_bump(val.clone(), est);
            }
            *self = TopKState::Hybrid { cms, heap, k };
        }
    }

    pub fn top(&self) -> Vec<(TopKValue, u64)> {
        match self {
            TopKState::Exact { counts, k, .. } => {
                let mut v: Vec<(TopKValue, u64)> =
                    counts.iter().map(|(k, c)| (k.clone(), *c)).collect();
                v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                v.truncate(*k);
                v
            }
            TopKState::Hybrid { heap, .. } => heap.top(),
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        match self {
            TopKState::Exact { counts, .. } => std::mem::size_of::<Self>() + counts.len() * 64,
            TopKState::Hybrid { cms, heap, .. } => cms.estimated_bytes() + heap.estimated_bytes(),
        }
    }

    /// Promote an Exact-mode state to Hybrid in place (no-op if already
    /// Hybrid). Same logic as the inline promotion inside `insert`,
    /// extracted so `merge` can reuse it.
    fn promote_to_hybrid(&mut self) {
        if matches!(self, TopKState::Hybrid { .. }) {
            return;
        }
        let TopKState::Exact {
            counts,
            k,
            hybrid_width,
            hybrid_depth,
            ..
        } = std::mem::replace(
            self,
            TopKState::Exact {
                counts: BTreeMap::new(),
                k: 1,
                threshold: 2,
                hybrid_width: 2048,
                hybrid_depth: 4,
            },
        )
        else {
            unreachable!("matches!(self, Hybrid) returned false above")
        };
        let mut cms = CountMinSketch::new(hybrid_width, hybrid_depth);
        let mut heap = TopKHeap::new(k);
        for (val, count) in counts.iter() {
            cms.update(val.hash64(), *count as i64);
        }
        for (val, _) in counts.iter() {
            let est = cms.estimate(val.hash64()).max(0) as u64;
            heap.insert_or_bump(val.clone(), est);
        }
        *self = TopKState::Hybrid { cms, heap, k };
    }

    /// Merge `other` into `self` so `self.top()` reflects the union of
    /// counts from both states.
    ///
    /// Used by the windowed-aggregation query path to combine per-bucket
    /// states across active buckets — without this, the windowed top_k
    /// query returns only the latest bucket's content (the prod bug
    /// observed on beava.dev's `top_page_1h` resetting every ~56s).
    ///
    /// Mode dispatch:
    /// * Exact + Exact: sum BTreeMap counts; promote to Hybrid if combined
    ///   distinct count exceeds the threshold.
    /// * Hybrid + Exact: fold other's counts into self's CMS, then refresh
    ///   the heap with the new estimates.
    /// * Exact + Hybrid: promote self to Hybrid, then recurse.
    /// * Hybrid + Hybrid: cell-wise CMS merge, then refresh the heap with
    ///   the combined values from both heaps reading the merged CMS.
    pub fn merge(&mut self, other: &TopKState) {
        // Exact + Hybrid: promote self up to Hybrid first, then continue.
        if matches!(self, TopKState::Exact { .. }) && matches!(other, TopKState::Hybrid { .. }) {
            self.promote_to_hybrid();
        }

        match (&mut *self, other) {
            (TopKState::Exact { counts: s, .. }, TopKState::Exact { counts: o, .. }) => {
                for (v, n) in o.iter() {
                    *s.entry(v.clone()).or_insert(0) += *n;
                }
            }
            (
                TopKState::Hybrid {
                    cms: s_cms,
                    heap: s_heap,
                    ..
                },
                TopKState::Exact { counts: o, .. },
            ) => {
                for (v, n) in o.iter() {
                    let h = v.hash64();
                    s_cms.update(h, *n as i64);
                }
                // Heap entries' counts may now under-report; refresh with
                // post-update estimates and add other's values.
                let self_vals: Vec<TopKValue> = s_heap.top().into_iter().map(|(v, _)| v).collect();
                for v in self_vals {
                    let est = s_cms.estimate(v.hash64()).max(0) as u64;
                    s_heap.insert_or_bump(v, est);
                }
                for (v, _) in o.iter() {
                    let est = s_cms.estimate(v.hash64()).max(0) as u64;
                    s_heap.insert_or_bump(v.clone(), est);
                }
            }
            (
                TopKState::Hybrid {
                    cms: s_cms,
                    heap: s_heap,
                    ..
                },
                TopKState::Hybrid {
                    cms: o_cms,
                    heap: o_heap,
                    ..
                },
            ) => {
                s_cms.merge(o_cms);
                let self_vals: Vec<TopKValue> = s_heap.top().into_iter().map(|(v, _)| v).collect();
                for v in self_vals {
                    let est = s_cms.estimate(v.hash64()).max(0) as u64;
                    s_heap.insert_or_bump(v, est);
                }
                for (v, _) in o_heap.top() {
                    let est = s_cms.estimate(v.hash64()).max(0) as u64;
                    s_heap.insert_or_bump(v, est);
                }
            }
            // (Exact, Hybrid) was handled by the promote-then-fall-through
            // branch above; this arm is unreachable in practice.
            (TopKState::Exact { .. }, TopKState::Hybrid { .. }) => {
                unreachable!("Exact+Hybrid should have been promoted before the match")
            }
        }

        // Post-merge promotion: if we're still Exact and the combined
        // distinct count crossed the threshold, promote.
        let need_promote = matches!(
            self,
            TopKState::Exact { counts, threshold, .. } if counts.len() > *threshold
        );
        if need_promote {
            self.promote_to_hybrid();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketches::cms::TopKValue;
    #[test]
    fn starts_in_exact_mode() {
        let s = TopKState::new(3, 1024, 2048, 4);
        assert_eq!(s.mode_name(), "v0_top_k_exact");
        assert_eq!(s.top(), Vec::<(TopKValue, u64)>::new());
    }
    #[test]
    fn exact_mode_returns_top_k() {
        let mut s = TopKState::new(3, 1024, 2048, 4);
        for _ in 0..100 {
            s.insert(TopKValue::Str("a".into()));
        }
        for _ in 0..50 {
            s.insert(TopKValue::Str("b".into()));
        }
        for _ in 0..30 {
            s.insert(TopKValue::Str("c".into()));
        }
        for _ in 0..10 {
            s.insert(TopKValue::Str("d".into()));
        }
        let top = s.top();
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].0, TopKValue::Str("a".into()));
        assert_eq!(top[0].1, 100);
        assert_eq!(top[1], (TopKValue::Str("b".into()), 50));
        assert_eq!(top[2], (TopKValue::Str("c".into()), 30));
    }
    #[test]
    fn promotes_to_hybrid_at_1024_distinct() {
        let mut s = TopKState::new(5, 1024, 2048, 4);
        for i in 0..1100 {
            s.insert(TopKValue::Str(format!("k{}", i)));
        }
        assert_eq!(s.mode_name(), "v0_top_k_hybrid");
    }
    #[test]
    fn heavy_hitters_correct_in_hybrid_mode() {
        let mut s = TopKState::new(3, 100, 2048, 4);
        for _ in 0..5000 {
            s.insert(TopKValue::Str("dominant".into()));
        }
        for j in 0..9 {
            for _ in 0..500 {
                s.insert(TopKValue::Str(format!("mid{}", j)));
            }
        }
        for i in 0..1000 {
            s.insert(TopKValue::Str(format!("noise{}", i)));
        }
        assert_eq!(s.mode_name(), "v0_top_k_hybrid");
        let top = s.top();
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].0, TopKValue::Str("dominant".into()));
        assert!(top[0].1 >= 5000);
        for (v, _) in &top[1..] {
            if let TopKValue::Str(s) = v {
                assert!(s.starts_with("mid"), "got {}", s);
            }
        }
    }
    #[test]
    fn promotion_preserves_dominant_key() {
        let mut s = TopKState::new(3, 100, 2048, 4);
        for _ in 0..1000 {
            s.insert(TopKValue::Str("dominant".into()));
        }
        for i in 0..200 {
            s.insert(TopKValue::Str(format!("k{}", i)));
        }
        assert_eq!(s.mode_name(), "v0_top_k_hybrid");
        let top = s.top();
        assert_eq!(top[0].0, TopKValue::Str("dominant".into()));
    }
    #[test]
    fn bincode_round_trip_exact() {
        let mut s = TopKState::new(3, 1024, 2048, 4);
        for _ in 0..10 {
            s.insert(TopKValue::Str("a".into()));
        }
        let bytes = bincode::serialize(&s).unwrap();
        let s2: TopKState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s2.mode_name(), "v0_top_k_exact");
        assert_eq!(s2.top(), s.top());
    }
    #[test]
    fn bincode_round_trip_hybrid() {
        let mut s = TopKState::new(3, 50, 2048, 4);
        for i in 0..100 {
            s.insert(TopKValue::Str(format!("k{}", i)));
        }
        for _ in 0..1000 {
            s.insert(TopKValue::Str("k0".into()));
        }
        assert_eq!(s.mode_name(), "v0_top_k_hybrid");
        let bytes = bincode::serialize(&s).unwrap();
        let s2: TopKState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s2.mode_name(), "v0_top_k_hybrid");
        assert_eq!(s2.top()[0].0, s.top()[0].0);
    }
    #[test]
    fn serde_tag_in_json() {
        let s = TopKState::new(3, 1024, 2048, 4);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("v0_top_k_exact"));
    }

    // ── merge ──────────────────────────────────────────────────────────
    //
    // Coverage gap pinned by these: the prod bug surfaced because the
    // windowed-aggregation query used to pick only the latest bucket, and
    // there was no `merge` method on TopKState to do better. These tests
    // pin the new merge contract so a regression can't sneak back in.

    #[test]
    fn merge_exact_plus_exact_sums_counts_per_value() {
        let mut a = TopKState::new(3, 1024, 2048, 4);
        let mut b = TopKState::new(3, 1024, 2048, 4);
        for _ in 0..5 {
            a.insert(TopKValue::Str("x".into()));
        }
        for _ in 0..3 {
            a.insert(TopKValue::Str("y".into()));
        }
        for _ in 0..7 {
            b.insert(TopKValue::Str("y".into()));
        }
        for _ in 0..2 {
            b.insert(TopKValue::Str("z".into()));
        }

        a.merge(&b);
        let top = a.top();
        // y: 3+7=10, x: 5, z: 2
        assert_eq!(top.len(), 3);
        assert_eq!(top[0], (TopKValue::Str("y".into()), 10));
        assert_eq!(top[1], (TopKValue::Str("x".into()), 5));
        assert_eq!(top[2], (TopKValue::Str("z".into()), 2));
        // Mode stays Exact — combined cardinality (3) is well below threshold.
        assert_eq!(a.mode_name(), "v0_top_k_exact");
    }

    #[test]
    fn merge_exact_plus_exact_promotes_and_preserves_counts() {
        // The Exact→Hybrid cutover during merge is the highest-risk path:
        // a buggy promotion could silently lose values and only the mode
        // name would flip. Make the test assert the dominant value's
        // count survives, not just that `mode_name` changed.
        //
        // Threshold = 5 → combined 12 distinct must promote to Hybrid.
        // "winner" appears in BOTH a (1000x) and b (500x) so the merge
        // must sum the BTreeMap entries before promoting; the promoted
        // Hybrid must then have CMS estimate ~1500 for "winner".
        let mut a = TopKState::new(2, 5, 2048, 4);
        let mut b = TopKState::new(2, 5, 2048, 4);
        // Each side: 5 distinct values total ("winner" + 4 fillers),
        // so counts.len() == threshold, the > comparison is false, and
        // they STAY Exact pre-merge. The merge brings combined to 9
        // distinct ("winner" plus 4+4 unique fillers), tripping the
        // threshold check and forcing the cutover.
        for _ in 0..1000 {
            a.insert(TopKValue::Str("winner".into()));
        }
        for i in 0..4 {
            a.insert(TopKValue::Str(format!("a{i}")));
        }
        for _ in 0..500 {
            b.insert(TopKValue::Str("winner".into()));
        }
        for i in 0..4 {
            b.insert(TopKValue::Str(format!("b{i}")));
        }
        assert_eq!(a.mode_name(), "v0_top_k_exact");
        assert_eq!(b.mode_name(), "v0_top_k_exact");

        a.merge(&b);

        // Mode flipped: 1 + 4 + 4 = 9 distinct values > threshold 5.
        assert_eq!(
            a.mode_name(),
            "v0_top_k_hybrid",
            "9 distinct values must promote across threshold=5"
        );
        // Counts preserved through the cutover: "winner" still on top.
        let top = a.top();
        assert_eq!(top[0].0, TopKValue::Str("winner".into()));
        assert!(
            top[0].1 >= 1500,
            "winner count must reflect the merged 1000+500 after promotion; got {}",
            top[0].1
        );
    }

    #[test]
    fn merge_hybrid_plus_hybrid_picks_dominant_key() {
        // Both sides force-promoted (threshold=5); each has a copy of
        // "dominant" in its heap. After merge, "dominant" must remain
        // top-1 with summed count.
        let mut a = TopKState::new(2, 5, 2048, 4);
        let mut b = TopKState::new(2, 5, 2048, 4);
        for _ in 0..1000 {
            a.insert(TopKValue::Str("dominant".into()));
        }
        for i in 0..10 {
            a.insert(TopKValue::Str(format!("a{i}")));
        }
        for _ in 0..500 {
            b.insert(TopKValue::Str("dominant".into()));
        }
        for i in 0..10 {
            b.insert(TopKValue::Str(format!("b{i}")));
        }
        assert_eq!(a.mode_name(), "v0_top_k_hybrid");
        assert_eq!(b.mode_name(), "v0_top_k_hybrid");
        a.merge(&b);
        let top = a.top();
        assert_eq!(top[0].0, TopKValue::Str("dominant".into()));
        assert!(
            top[0].1 >= 1500,
            "dominant must reflect both contributions; got {}",
            top[0].1
        );
    }

    #[test]
    fn merge_exact_plus_hybrid_via_promotion() {
        let mut a = TopKState::new(2, 1024, 2048, 4);
        for _ in 0..50 {
            a.insert(TopKValue::Str("shared".into()));
        }
        let mut b = TopKState::new(2, 5, 2048, 4);
        for _ in 0..200 {
            b.insert(TopKValue::Str("shared".into()));
        }
        for i in 0..10 {
            b.insert(TopKValue::Str(format!("b{i}")));
        }
        assert_eq!(a.mode_name(), "v0_top_k_exact");
        assert_eq!(b.mode_name(), "v0_top_k_hybrid");
        a.merge(&b);
        assert_eq!(a.mode_name(), "v0_top_k_hybrid");
        let top = a.top();
        assert_eq!(top[0].0, TopKValue::Str("shared".into()));
        assert!(
            top[0].1 >= 250,
            "shared must reflect 50 from self + 200 from other; got {}",
            top[0].1
        );
    }

    #[test]
    fn merge_hybrid_plus_exact_folds_counts() {
        let mut a = TopKState::new(2, 5, 2048, 4);
        for _ in 0..1000 {
            a.insert(TopKValue::Str("x".into()));
        }
        for i in 0..10 {
            a.insert(TopKValue::Str(format!("a{i}")));
        }
        let mut b = TopKState::new(2, 1024, 2048, 4);
        for _ in 0..500 {
            b.insert(TopKValue::Str("x".into()));
        }
        for _ in 0..200 {
            b.insert(TopKValue::Str("y".into()));
        }
        assert_eq!(a.mode_name(), "v0_top_k_hybrid");
        assert_eq!(b.mode_name(), "v0_top_k_exact");
        a.merge(&b);
        let top = a.top();
        assert_eq!(top[0].0, TopKValue::Str("x".into()));
        assert!(
            top[0].1 >= 1500,
            "x must reflect 1000+500; got {}",
            top[0].1
        );
        assert!(
            top.iter()
                .any(|(v, _)| matches!(v, TopKValue::Str(s) if s == "y")),
            "y (count 200) should be in the merged top-2"
        );
    }

    #[test]
    fn merge_into_empty_self_yields_other() {
        let mut a = TopKState::new(2, 1024, 2048, 4);
        let mut b = TopKState::new(2, 1024, 2048, 4);
        for _ in 0..5 {
            b.insert(TopKValue::Str("x".into()));
        }
        a.merge(&b);
        assert_eq!(a.top(), vec![(TopKValue::Str("x".into()), 5)]);
    }
}
