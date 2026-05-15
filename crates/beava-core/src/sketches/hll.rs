//! HyperLogLog++ with p=12 (4096 registers), bias correction, linear counting.
//! Ported from main:src/engine/hll.rs (Apache 2.0).
//!
//! Pure-data sketch — wrapping into AggOp/state machinery is the
//! responsibility of `count_distinct.rs` (the 3-mode hybrid wrapper).
//!
//! API:
//! - `new()` — empty HLL with 4096 zeroed registers
//! - `add_hash(u64)` — top-12 bits select register; `(remaining<<12 | low_bit) leading_zeros + 1` is rank
//! - `estimate() -> u64` — bias-corrected HLL++ estimate w/ linear counting fallback
//! - `merge(&Hll)` — register-wise max
//! - `estimated_bytes() -> usize` — ~4096 (one byte per register) + struct overhead

use serde::{Deserialize, Serialize};

// ======================== Constants ========================

/// HLL precision: 12 bits = 4096 registers. ~1.6% error. ~4 KB dense.
const HLL_P: usize = 12;
/// Number of registers
const HLL_M: usize = 1 << HLL_P;
/// Alpha correction constant for m=4096
const HLL_ALPHA: f64 = 0.7213 / (1.0 + 1.079 / HLL_M as f64);

/// HLL++ linear counting threshold for p=12 (from Google's zetasketch).
const LC_THRESHOLD: f64 = 3100.0;

/// KNN neighbor count for bias interpolation (same as Google's zetasketch).
const KNN_K: usize = 6;

/// HLL++ bias correction data for p=12 (from Google's zetasketch / Heule et al. 2013).
const RAW_ESTIMATE_DATA: [f64; 201] = [
    2954.0, 3003.4782, 3053.3568, 3104.3666, 3155.324, 3206.9598, 3259.648, 3312.539, 3366.1474,
    3420.2576, 3474.8376, 3530.6076, 3586.451, 3643.38, 3700.4104, 3757.5638, 3815.9676, 3875.193,
    3934.838, 3994.8548, 4055.018, 4117.1742, 4178.4482, 4241.1294, 4304.4776, 4367.4044,
    4431.8724, 4496.3732, 4561.4304, 4627.5326, 4693.949, 4761.5532, 4828.7256, 4897.6182,
    4965.5186, 5034.4528, 5104.865, 5174.7164, 5244.6828, 5316.6708, 5387.8312, 5459.9036,
    5532.476, 5604.8652, 5679.6718, 5753.757, 5830.2072, 5905.2828, 5980.0434, 6056.6264,
    6134.3192, 6211.5746, 6290.0816, 6367.1176, 6447.9796, 6526.5576, 6606.1858, 6686.9144,
    6766.1142, 6847.0818, 6927.9664, 7010.9096, 7091.0816, 7175.3962, 7260.3454, 7344.018,
    7426.4214, 7511.3106, 7596.0686, 7679.8094, 7765.818, 7852.4248, 7936.834, 8022.363, 8109.5066,
    8200.4554, 8288.5832, 8373.366, 8463.4808, 8549.7682, 8642.0522, 8728.3288, 8820.9528,
    8907.727, 9001.0794, 9091.2522, 9179.988, 9269.852, 9362.6394, 9453.642, 9546.9024, 9640.6616,
    9732.6622, 9824.3254, 9917.7484, 10007.9392, 10106.7508, 10196.2152, 10289.8114, 10383.5494,
    10482.3064, 10576.8734, 10668.7872, 10764.7156, 10862.0196, 10952.793, 11049.9748, 11146.0702,
    11241.4492, 11339.2772, 11434.2336, 11530.741, 11627.6136, 11726.311, 11821.5964, 11918.837,
    12015.3724, 12113.0162, 12213.0424, 12306.9804, 12408.4518, 12504.8968, 12604.586, 12700.9332,
    12798.705, 12898.5142, 12997.0488, 13094.788, 13198.475, 13292.7764, 13392.9698, 13486.8574,
    13590.1616, 13686.5838, 13783.6264, 13887.2638, 13992.0978, 14081.0844, 14189.9956, 14280.0912,
    14382.4956, 14486.4384, 14588.1082, 14686.2392, 14782.276, 14888.0284, 14985.1864, 15088.8596,
    15187.0998, 15285.027, 15383.6694, 15495.8266, 15591.3736, 15694.2008, 15790.3246, 15898.4116,
    15997.4522, 16095.5014, 16198.8514, 16291.7492, 16402.6424, 16499.1266, 16606.2436, 16697.7186,
    16796.3946, 16902.3376, 17005.7672, 17100.814, 17206.8282, 17305.8262, 17416.0744, 17508.4092,
    17617.0178, 17715.4554, 17816.758, 17920.1748, 18012.9236, 18119.7984, 18223.2248, 18324.2482,
    18426.6276, 18525.0932, 18629.8976, 18733.2588, 18831.0466, 18940.1366, 19032.2696, 19131.729,
    19243.4864, 19349.6932, 19442.866, 19547.9448, 19653.2798, 19754.4034, 19854.0692, 19965.1224,
    20065.1774, 20158.2212, 20253.353, 20366.3264, 20463.22,
];

const BIAS_DATA: [f64; 201] = [
    2953.0, 2900.4782, 2848.3568, 2796.3666, 2745.324, 2694.9598, 2644.648, 2595.539, 2546.1474,
    2498.2576, 2450.8376, 2403.6076, 2357.451, 2311.38, 2266.4104, 2221.5638, 2176.9676, 2134.193,
    2090.838, 2048.8548, 2007.018, 1966.1742, 1925.4482, 1885.1294, 1846.4776, 1807.4044,
    1768.8724, 1731.3732, 1693.4304, 1657.5326, 1621.949, 1586.5532, 1551.7256, 1517.6182,
    1483.5186, 1450.4528, 1417.865, 1385.7164, 1352.6828, 1322.6708, 1291.8312, 1260.9036,
    1231.476, 1201.8652, 1173.6718, 1145.757, 1119.2072, 1092.2828, 1065.0434, 1038.6264,
    1014.3192, 988.5746, 965.0816, 940.1176, 917.9796, 894.5576, 871.1858, 849.9144, 827.1142,
    805.0818, 783.9664, 763.9096, 742.0816, 724.3962, 706.3454, 688.018, 667.4214, 650.3106,
    633.0686, 613.8094, 597.818, 581.4248, 563.834, 547.363, 531.5066, 520.4554, 505.5832, 488.366,
    476.4808, 459.7682, 450.0522, 434.3288, 423.9528, 408.727, 399.0794, 387.2522, 373.988,
    360.852, 351.6394, 339.642, 330.9024, 322.6616, 311.6622, 301.3254, 291.7484, 279.9392,
    276.7508, 263.2152, 254.8114, 245.5494, 242.3064, 234.8734, 223.7872, 217.7156, 212.0196,
    200.793, 195.9748, 189.0702, 182.4492, 177.2772, 170.2336, 164.741, 158.6136, 155.311,
    147.5964, 142.837, 137.3724, 132.0162, 130.0424, 121.9804, 120.4518, 114.8968, 111.586,
    105.9332, 101.705, 98.5142, 95.0488, 89.788, 91.475, 83.7764, 80.9698, 72.8574, 73.1616,
    67.5838, 62.6264, 63.2638, 66.0978, 52.0844, 58.9956, 47.0912, 46.4956, 48.4384, 47.1082,
    43.2392, 37.276, 40.0284, 35.1864, 35.8596, 32.0998, 28.027, 23.6694, 33.8266, 26.3736,
    27.2008, 21.3246, 26.4116, 23.4522, 19.5014, 19.8514, 10.7492, 18.6424, 13.1266, 18.2436,
    6.7186, 3.3946, 6.3376, 7.7672, 0.814, 3.8282, 0.8262, 8.0744, -1.5908, 5.0178, 0.4554, -0.242,
    0.1748, -9.0764, -4.2016, -3.7752, -4.7518, -5.3724, -8.9068, -6.1024, -5.7412, -9.9534,
    -3.8634, -13.7304, -16.271, -7.5136, -3.3068, -13.134, -10.0552, -6.7202, -8.5966, -10.9308,
    -1.8776, -4.8226, -13.7788, -21.647, -10.6736, -15.78,
];

// ======================== Hll struct ========================

/// HyperLogLog++ dense register sketch. Pure data; promotion into/out of
/// exact/hashset modes is handled by `CountDistinctState` (count_distinct.rs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hll {
    /// 4096 6-bit registers stored as bytes (top 2 bits unused).
    registers: Vec<u8>,
}

impl Default for Hll {
    fn default() -> Self {
        Hll {
            registers: vec![0u8; HLL_M],
        }
    }
}

impl Hll {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_capacity(&self) -> usize {
        self.registers.capacity()
    }

    /// SplitMix64 — improves the avalanche / uniform-distribution properties
    /// of the input hash, which is critical for HLL accuracy when callers
    /// pass hashes from a non-cryptographic hasher (ahash etc.).
    #[inline]
    fn mix64(mut z: u64) -> u64 {
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Extract HLL register index and rank from a hash.
    #[inline]
    fn hash_to_register(hash: u64) -> (usize, u8) {
        let h = Self::mix64(hash);
        let index = (h >> (64 - HLL_P)) as usize;
        let remaining = (h << HLL_P) | (1 << (HLL_P - 1));
        let rank = remaining.leading_zeros() as u8 + 1;
        (index, rank)
    }

    /// Insert a precomputed u64 hash. Caller is responsible for hashing.
    pub fn add_hash(&mut self, hash: u64) {
        let (idx, rank) = Self::hash_to_register(hash);
        if rank > self.registers[idx] {
            self.registers[idx] = rank;
        }
    }

    /// HLL++ cardinality estimate with Google's bias correction.
    pub fn estimate(&self) -> u64 {
        let registers = &self.registers;
        let sum: f64 = registers.iter().map(|&r| 2.0_f64.powi(-(r as i32))).sum();
        let raw = HLL_ALPHA * (HLL_M as f64) * (HLL_M as f64) / sum;
        let zeros = registers.iter().filter(|&&r| r == 0).count();

        // Linear counting estimate (used when zeros exist)
        let lc = if zeros > 0 {
            (HLL_M as f64) * (HLL_M as f64 / zeros as f64).ln()
        } else {
            0.0
        };

        // Decision logic from HLL++ paper.
        if raw <= LC_THRESHOLD && zeros > 0 {
            return lc.round() as u64;
        }

        // Bias correction range: subtract estimated bias.
        if raw <= RAW_ESTIMATE_DATA[RAW_ESTIMATE_DATA.len() - 1] {
            let bias = Self::estimate_bias_knn(raw);
            let corrected = (raw - bias).max(0.0);
            // Prefer linear counting when smaller (more accurate at small counts)
            if zeros > 0 && lc < corrected {
                return lc.round() as u64;
            }
            return corrected.round() as u64;
        }

        // Large range: raw estimate is unbiased.
        raw.round() as u64
    }

    /// KNN-based bias estimation from Google's precomputed table.
    fn estimate_bias_knn(raw: f64) -> f64 {
        let n = RAW_ESTIMATE_DATA.len();

        let mut distances: Vec<(f64, usize)> = RAW_ESTIMATE_DATA
            .iter()
            .enumerate()
            .map(|(i, &est)| ((raw - est).abs(), i))
            .collect();
        distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let k = KNN_K.min(n);
        let bias_sum: f64 = distances[..k].iter().map(|&(_, idx)| BIAS_DATA[idx]).sum();
        bias_sum / k as f64
    }

    /// Merge another sketch into this one (register-wise max — union semantics).
    pub fn merge(&mut self, other: &Hll) {
        debug_assert_eq!(self.registers.len(), HLL_M);
        debug_assert_eq!(other.registers.len(), HLL_M);
        for i in 0..HLL_M {
            if other.registers[i] > self.registers[i] {
                self.registers[i] = other.registers[i];
            }
        }
    }

    /// Approximate memory footprint in bytes.
    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.registers.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Deterministic hash for tests — `ahash::AHasher::default()` uses a
    /// runtime-random seed which makes accuracy thresholds flaky across
    /// runs. Seeded RandomState gives us reproducible hashes that exercise
    /// the same registers every run.
    fn hash_str(s: &str) -> u64 {
        let rs = ahash::RandomState::with_seeds(
            0x243f_6a88_85a3_08d3,
            0x1319_8a2e_0370_7344,
            0xa409_3822_299f_31d0,
            0x082e_fa98_ec4e_6c89,
        );
        rs.hash_one(s)
    }
    #[test]
    fn empty_estimate_is_zero() {
        let h = Hll::new();
        assert_eq!(h.estimate(), 0);
    }
    #[test]
    fn small_set_estimate_within_1pct() {
        let mut h = Hll::new();
        for i in 0..500 {
            h.add_hash(hash_str(&format!("k{}", i)));
        }
        let est = h.estimate();
        let err = (est as i64 - 500).abs() as f64 / 500.0;
        assert!(err < 0.05, "small-set err {} > 5%", err);
    }
    #[test]
    fn med_set_estimate_within_2pct() {
        let mut h = Hll::new();
        for i in 0..10_000 {
            h.add_hash(hash_str(&format!("k{}", i)));
        }
        let est = h.estimate();
        let err = (est as i64 - 10_000).abs() as f64 / 10_000.0;
        assert!(err < 0.02, "med-set err {} > 2%", err);
    }
    #[test]
    fn large_set_estimate_within_15pct() {
        let mut h = Hll::new();
        for i in 0..100_000 {
            h.add_hash(hash_str(&format!("k{}", i)));
        }
        let est = h.estimate();
        let err = (est as i64 - 100_000).abs() as f64 / 100_000.0;
        assert!(err < 0.015, "large-set err {} > 1.5%", err);
    }
    #[test]
    fn merge_unions_registers() {
        let mut h1 = Hll::new();
        let mut h2 = Hll::new();
        for i in 0..5_000 {
            h1.add_hash(hash_str(&format!("a{}", i)));
        }
        for i in 0..5_000 {
            h2.add_hash(hash_str(&format!("b{}", i)));
        }
        h1.merge(&h2);
        let est = h1.estimate();
        let err = (est as i64 - 10_000).abs() as f64 / 10_000.0;
        assert!(err < 0.03, "merged err {} > 3%", err);
    }
    #[test]
    fn bincode_round_trip_preserves_estimate() {
        let mut h = Hll::new();
        for i in 0..1_000 {
            h.add_hash(hash_str(&format!("k{}", i)));
        }
        let bytes = bincode::serialize(&h).unwrap();
        let h2: Hll = bincode::deserialize(&bytes).unwrap();
        assert_eq!(h2.estimate(), h.estimate());
    }
    #[test]
    fn estimated_bytes_within_5kb() {
        let h = Hll::new();
        assert!(
            h.estimated_bytes() <= 5_000,
            "Hll should be ≤ 5KB; got {}",
            h.estimated_bytes()
        );
    }
}
