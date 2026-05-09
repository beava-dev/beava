//! PercentileState: 2-mode hybrid (Exact Vec<f64> ≤256 → UDDSketch).
//! Serde rename tags `v0_percentile_exact` / `v0_percentile_uddsketch`
//! for snapshot stability across versions.

use crate::sketches::uddsketch::{UDDSketch, DEFAULT_MAX_BUCKETS};
use serde::{Deserialize, Serialize};

const DEFAULT_EXACT_THRESHOLD: usize = 256;

/// Externally-tagged so `bincode` can round-trip (`#[serde(tag = ...)]`
/// internally-tagged enums require `deserialize_any`, which bincode lacks).
/// JSON output is `{"v0_percentile_exact": {...}}` — the rename tag still
/// appears in serialized form, satisfying snapshot tag-stability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PercentileState {
    #[serde(rename = "v0_percentile_exact")]
    Exact {
        values: Vec<f64>,
        threshold: usize,
        alpha0: f64,
    },
    #[serde(rename = "v0_percentile_uddsketch")]
    Sketch { sketch: UDDSketch },
}

impl PercentileState {
    /// Construct in Exact mode. Promotes to UDDSketch once `exact_threshold`
    /// values have been inserted.
    pub fn new(exact_threshold: usize, alpha0: f64) -> Self {
        let threshold = exact_threshold.max(2);
        PercentileState::Exact {
            values: Vec::with_capacity(threshold.max(DEFAULT_EXACT_THRESHOLD)),
            threshold,
            alpha0,
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            PercentileState::Exact { .. } => "v0_percentile_exact",
            PercentileState::Sketch { .. } => "v0_percentile_uddsketch",
        }
    }

    pub fn insert(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        let need_promote = match self {
            PercentileState::Exact {
                values, threshold, ..
            } => {
                values.push(value);
                values.len() > *threshold
            }
            PercentileState::Sketch { sketch } => {
                sketch.insert(value);
                false
            }
        };
        if need_promote {
            if let PercentileState::Exact { values, alpha0, .. } = self {
                let mut sketch = UDDSketch::new(*alpha0, DEFAULT_MAX_BUCKETS);
                for v in values.iter() {
                    sketch.insert(*v);
                }
                *self = PercentileState::Sketch { sketch };
            }
        }
    }

    pub fn quantile(&self, q: f64) -> Option<f64> {
        match self {
            PercentileState::Exact { values, .. } => {
                if values.is_empty() {
                    return None;
                }
                let mut sorted: Vec<f64> = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let pos = q * (sorted.len() as f64 - 1.0);
                let lo = pos.floor() as usize;
                let hi = pos.ceil() as usize;
                if lo == hi {
                    Some(sorted[lo])
                } else {
                    let frac = pos - lo as f64;
                    Some(sorted[lo] * (1.0 - frac) + sorted[hi] * frac)
                }
            }
            PercentileState::Sketch { sketch } => sketch.quantile(q),
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        match self {
            PercentileState::Exact { values, .. } => {
                std::mem::size_of::<Self>() + values.capacity() * 8
            }
            PercentileState::Sketch { sketch } => sketch.estimated_bytes(),
        }
    }

    /// Promote an Exact-mode state to Sketch in place. No-op if already a
    /// Sketch.
    fn promote_to_sketch(&mut self) {
        if matches!(self, PercentileState::Sketch { .. }) {
            return;
        }
        if let PercentileState::Exact { values, alpha0, .. } = self {
            let mut sketch = UDDSketch::new(*alpha0, DEFAULT_MAX_BUCKETS);
            for v in values.iter() {
                sketch.insert(*v);
            }
            *self = PercentileState::Sketch { sketch };
        }
    }

    /// Merge `other` into `self` so `self.quantile(q)` reflects the union
    /// of values from both states. Used by the windowed-aggregation query
    /// path so a windowed `quantile()` aggregates across all active
    /// buckets instead of returning only the latest one's quantile.
    pub fn merge(&mut self, other: &PercentileState) {
        // If either side is sketch-mode, promote both to sketch and merge
        // via UDDSketch::merge. Otherwise (Exact + Exact), append values
        // and let any over-threshold accumulation promote at the end.
        if matches!(other, PercentileState::Sketch { .. }) {
            self.promote_to_sketch();
        }

        match (&mut *self, other) {
            (
                PercentileState::Exact { values: s, .. },
                PercentileState::Exact { values: o, .. },
            ) => {
                s.extend(o.iter().copied());
            }
            (
                PercentileState::Sketch { sketch: s_sk },
                PercentileState::Sketch { sketch: o_sk },
            ) => {
                s_sk.merge(o_sk);
            }
            (
                PercentileState::Sketch { sketch: s_sk },
                PercentileState::Exact { values: o, .. },
            ) => {
                for v in o.iter() {
                    s_sk.insert(*v);
                }
            }
            (PercentileState::Exact { .. }, PercentileState::Sketch { .. }) => {
                unreachable!("promote_to_sketch above ensures self is Sketch when other is Sketch")
            }
        }

        // Post-merge: promote if Exact accumulated past threshold.
        let need_promote = matches!(
            self,
            PercentileState::Exact { values, threshold, .. }
                if values.len() > *threshold
        );
        if need_promote {
            self.promote_to_sketch();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starts_in_exact_mode() {
        let s = PercentileState::new(256, 0.01);
        assert_eq!(s.mode_name(), "v0_percentile_exact");
        assert!(s.quantile(0.5).is_none());
    }
    #[test]
    fn exact_mode_quantile_is_exact() {
        let mut s = PercentileState::new(256, 0.01);
        for v in 1..=100 {
            s.insert(v as f64);
        }
        let p50 = s.quantile(0.5).unwrap();
        assert!((p50 - 50.5).abs() < 1.0, "p50={}", p50);
    }
    #[test]
    fn promotes_to_sketch_at_threshold() {
        let mut s = PercentileState::new(256, 0.01);
        for i in 1..=300 {
            s.insert(i as f64);
        }
        assert_eq!(s.mode_name(), "v0_percentile_uddsketch");
        let p99 = s.quantile(0.99).unwrap();
        let err = (p99 - 297.0).abs() / 297.0;
        assert!(err < 0.05, "p99={} err={}", p99, err);
    }
    #[test]
    fn promotion_preserves_quantile_close_to_exact() {
        let mut e = PercentileState::new(256, 0.01);
        for v in 1..=200 {
            e.insert(v as f64);
        }
        let p50_exact = e.quantile(0.5).unwrap();
        for v in 201..=300 {
            e.insert(v as f64);
        }
        assert_eq!(e.mode_name(), "v0_percentile_uddsketch");
        let p50_after = e.quantile(0.5).unwrap();
        assert!((p50_after - 150.0).abs() / 150.0 < 0.05);
        assert!((p50_exact - 100.5).abs() < 1.0);
    }
    #[test]
    fn bincode_round_trip_exact() {
        let mut s = PercentileState::new(256, 0.01);
        for v in 1..=50 {
            s.insert(v as f64);
        }
        let bytes = bincode::serialize(&s).unwrap();
        let s2: PercentileState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s2.mode_name(), "v0_percentile_exact");
        assert!((s.quantile(0.5).unwrap() - s2.quantile(0.5).unwrap()).abs() < 1e-9);
    }
    #[test]
    fn bincode_round_trip_sketch() {
        let mut s = PercentileState::new(256, 0.01);
        for v in 1..=1_000 {
            s.insert(v as f64);
        }
        assert_eq!(s.mode_name(), "v0_percentile_uddsketch");
        let bytes = bincode::serialize(&s).unwrap();
        let s2: PercentileState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s2.mode_name(), "v0_percentile_uddsketch");
        assert!((s.quantile(0.5).unwrap() - s2.quantile(0.5).unwrap()).abs() < 1e-9);
    }
    #[test]
    fn serde_tag_in_json() {
        let mut s = PercentileState::new(256, 0.01);
        s.insert(1.0);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("v0_percentile_exact"));
    }
}
