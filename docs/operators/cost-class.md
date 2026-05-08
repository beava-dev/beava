# Operator Cost-Class Catalogue

**Updated:** 2026-04-27.
**Source:** `.planning/research/operator-update-uniformity-audit.md`
**Catalogue size:** 53 ops (38 Tier 1 + 6 Tier 2 + 9 Tier 3) — post-cleanup
of `bv.unique_cells` + `bv.geo_entropy` (both Tier 2 per uniformity-audit lines 173-174 + 206).

---

## How to read this table

- **Tier 1 (≤30-40 ns/call post-Phase-19.2):** Plain register-arithmetic or direct array
  writes. Update is a register write or a small arithmetic step. Use freely; cost scales
  linearly with feature count.
- **Tier 2 (30-100 ns/call):** Small fixed-cost work above the register baseline — hashing,
  sqrt, haversine, small bounded data-structure access (≤256 entries). Inexpensive but not
  free; budget proportionally.
- **Tier 3 (100-300 ns/call):** Algorithmic floor that traverses an unbounded-cardinality
  data structure, or a data-structure with non-trivial per-event cost (BTreeMap key insert,
  heap sift, Value clone through cold cache). Use for the 1-3 fraud features that genuinely
  need them; not free.

Per-call costs assume the apply-path is hot (warm-key, post-pre-extraction,
post-hasher-cache). Cold-key apply is ~150-200 ns higher per op due to per-entity
initialization (mitigated by v0's lazy-bucket fix).

**Windowed operators** delegate to an inner op; their cost is the inner op's tier cost plus
~5-10 ns bucket dispatch. See the inner op row for the real cost class.

---

## Tier 1: Fast (≤40 ns/call) — 38 ops

**Tier 1 derivation:** 8 core + 15 point/recency/streak + 14 decay/velocity (15 minus OutlierCount) + 1 z-score = 38. (HourOfDayHistogram,
DowHourHistogram, SeasonalDeviation also listed here as time-bucket ops with Tier 1 update
floors; Histogram, MostRecentN, ReservoirSample, DistanceFromHome, GeoSpread are placed
in Tier 3 due to query-path or clone-path concerns — see notes there.)

**Correction vs raw audit Tier 1 list:** The audit's Tier 1 explicit list (lines 183-187)
includes 45 names. Of those, GeoVelocity + GeoDistance are Tier 2 (haversine floor), and
Histogram + MostRecentN + ReservoirSample + DistanceFromHome + GeoSpread are Tier 3 in
this catalogue (per plan resolution). 45 - 2 Tier 2 - 5 Tier 3 = **38 Tier 1 ops** ✓

| Op | Family | Algo floor | Post-19.2 expected | Notes |
|---|---|---|---|---|
| `bv.count` | v0 / core | 5 ns | 25 ns | Pure `n += 1`. Trace overhead today is wrapping (~270 ns), not the algorithm. |
| `bv.sum` | v0 / core | 8 ns | 25 ns | `total += v; n += 1`. |
| `bv.mean` | v0 / core | 8 ns | 25 ns | Running sum + count; query divides. Previously called `avg` (renamed in ADR-002). |
| `bv.min` | v0 / core | 10 ns | 30 ns | Compare + Value clone on string keys. |
| `bv.max` | v0 / core | 10 ns | 30 ns | Mirror of min. |
| `bv.var` | v0 / core | 12 ns | 32 ns | Welford online second-moment (5 FP ops). Previously called `variance` (renamed in ADR-002). |
| `bv.std` | v0 / core | 12 ns | 32 ns | Same state as `var`; sqrt deferred to query. Previously called `stddev` (renamed in ADR-002). |
| `bv.ratio` | v0 / core | 5 ns | 25 ns | `total += 1; if cond: matching += 1`. |
| `bv.first` | v0 / point | 5 ns | 25 ns | Early-exit once `current.is_some()`. |
| `bv.last` | v0 / point | 8 ns | 30 ns | Single Value clone per accepted event. |
| `bv.first_n` | v0 / point | 8 ns | 30 ns | Vec push until `len >= n` then no-op (n bounded at register-time). |
| `bv.last_n` | v0 / point | 10 ns | 32 ns | VecDeque push_back + pop_front when full. |
| `bv.lag` | v0 / point | 10 ns | 32 ns | VecDeque ring of capacity n+1; bounded. |
| `bv.first_seen` | v0 / recency | 8 ns | 30 ns | Two Option<i64> writes. |
| `bv.last_seen` | v0 / recency | 8 ns | 30 ns | Shares SeenState with first_seen. |
| `bv.age` | v0 / recency | 8 ns | 30 ns | Same update; query subtracts query_time. |
| `bv.has_seen` | v0 / recency | 4 ns | 25 ns | Boolean flag write. |
| `bv.time_since` | v0 / recency | 8 ns | 30 ns | High-variance bench (quiescent noise), not a real algo cost. |
| `bv.time_since_last_n` | v0 / recency | 12 ns | 35 ns | VecDeque ring of capacity n; bounded. |
| `bv.streak` | v0 / streak | 10 ns | 30 ns | Two u64 writes. |
| `bv.max_streak` | v0 / streak | 12 ns | 32 ns | Same state struct as streak. |
| `bv.negative_streak` | v0 / streak | 10 ns | 30 ns | Inverse of streak. |
| `bv.first_seen_in_window` | v0 / windowed-recency | 8 ns | 30 ns | Single `last_ms` write; query computes age vs window. |
| `bv.ewma` | v0 / decay | 15 ns | 35 ns | One `exp()` + 3 scalar muls. |
| `bv.ewvar` | v0 / decay | 18 ns | 38 ns | EW-Welford: one exp + 5 scalar ops. |
| `bv.ew_zscore` | v0 / decay | 18 ns | 38 ns | Wraps ewvar; computes z-score at query. |
| `bv.decayed_sum` | v0 / decay | 15 ns | 35 ns | Cormode forward-decay; one exp. |
| `bv.decayed_count` | v0 / decay | 12 ns | 32 ns | No field — fastest decay op. |
| `bv.twa` | v0 / decay | 15 ns | 35 ns | Time-weighted average; 4 scalar updates. |
| `bv.rate_of_change` | v0 / velocity | 10 ns | 30 ns | Delta value / delta time scalar math. |
| `bv.inter_arrival_stats` | v0 / velocity | 12 ns | 32 ns | Welford on inter-arrival gaps. |
| `bv.burst_count` | v0 / velocity | 12 ns | 32 ns | Bounded 64-bucket sliding sub-window (O(1) modulo index). |
| `bv.delta_from_prev` | v0 / velocity | 8 ns | 28 ns | Two scalar writes. |
| `bv.trend` | v0 / velocity | 12 ns | 32 ns | Online OLS — 4 running sums. |
| `bv.trend_residual` | v0 / velocity | 16 ns | 36 ns | Wraps trend; residual computed at query. |
| `bv.value_change_count` | v0 / velocity | 8 ns | 28 ns | One scalar compare. |
| `bv.zscore` | v0 / z-score | 18 ns | 38 ns | Welford + sqrt deferred to query. |
| `bv.hour_of_day_histogram` | v0 / buffer | 4 ns | 25 ns | Direct `[u64; 24]` array index — fastest time-bucket op. |
| `bv.dow_hour_histogram` | v0 / buffer | 4 ns | 25 ns | Direct Vec[168] index. |
| `bv.seasonal_deviation` | v0 / buffer | 10 ns | 30 ns | Per-hour bucket update: `n += 1; sum += v; sum_sq += v*v`. |

---

## Tier 2: Moderate (30-100 ns/call) — 6 ops

These ops have a fixed-cost algorithmic floor above the register-arithmetic baseline:
hashing, sqrt, haversine geometry, or small bounded data-structure access.

| Op | Family | Algo floor | Post-19.2 expected | Notes |
|---|---|---|---|---|
| `bv.outlier_count` | v0 / velocity | 22 ns | 42 ns | Welford + 1 sqrt per event. The sqrt is the irreducible Tier 2 cost (no path eliminates it). |
| `bv.n_unique` | v0 / sketch | 18-80 ns | 50-130 ns | Mode-dependent: HLL (~18 ns algo, ~80 ns post-wrapping-fix), HashSet (~80 ns), ExactArray (~12 ns). Runtime mode selection; API is uniform. HLL mode applies after 1024 distinct values. Previously called `count_distinct` (renamed in ADR-002). |
| `bv.bloom_member` | v0 / sketch | 35 ns | 70 ns | 7 hashes x 7 bit-sets (k=7 for fpr=0.01). Fixed k regardless of entity history. |
| `bv.geo_velocity` | v0 / geo | 20 ns | 45 ns | Two scalar reads + haversine (sin/cos/sqrt of two trig identities). |
| `bv.geo_distance` | v0 / geo | 18 ns | 42 ns | Same haversine floor as geo_velocity; simpler accumulation step. |
| `bv.quantile` (Exact mode) | v0 / sketch | 8 ns | 35 ns | **Dual-mode:** Exact mode (up to 256 events) is Tier 2. After UDDSketch promotion the op moves to Tier 3 (see below). Fraud pipelines with low-cardinality entity keys often stay in Exact mode and see Tier 2 costs throughout production lifetime. Previously called `percentile` (renamed in ADR-002). |

---

## Tier 3: Algorithmic floor (100-300 ns/call) — 9 ops

These ops traverse an unbounded-cardinality data structure or have non-trivial per-event
overhead (BTreeMap key insert, heap log-k sift, Value clone through cold cache). The
algorithmic floor IS what delivers correctness guarantees — no template change erases it.

| Op | Family | Algo floor | Post-19.2 expected | Notes |
|---|---|---|---|---|
| `bv.quantile` (UDDSketch mode) | v0 / sketch | 130 ns | 180 ns | BTreeMap log2(2048) ~11-level walk + entry update + occasional collapse. A planned change replaces BTreeMap with flat sorted Vec; new projected floor ~75 ns. Kicks in after Exact-mode promotion (>256 distinct values). Previously called `percentile` (renamed in ADR-002). |
| `bv.top_k` | v0 / sketch | 250 ns | 300 ns | Hybrid mode: CMS 4-row hash updates + heap log-k sift via HashMap side-index. Exact mode (<=1024 distinct) is ~95 ns. Default mode is Hybrid for large-k or high-cardinality fraud pipelines. |
| `bv.entropy` | v0 / sketch | 60 ns | 160 ns | BTreeMap key insert + cap-and-drop when full (max_categories=1024 default, an internal plan D-05a). String-key allocation is irreducible. BTreeMap walk is log(min(distinct, 1024)). |
| `bv.event_type_mix` | v0 / buffer | 70 ns | 150 ns | BTreeMap on String key + AHashSet allowlist check (O(1) post-Plan-19.2-05 fix from Vec linear scan). Pre-fix trace: 1,127 ns/call. Post-fix floor: ~100-150 ns (BTreeMap key insert irreducible). |
| `bv.histogram` | v0 / buffer | 10 ns (update) | 30 ns (update) | UPDATE is fast (linear scan <=20 buckets, Tier 1 floor). Listed here because QUERY allocates a map (cold path). Flag the asymmetry when profiling: apply-thread cost is Tier 1; query-time cost is higher. |
| `bv.most_recent_n` | v0 / buffer | 12 ns | 32 ns | Circular buffer of capacity n; one Value clone per event. Value::Str clone = Arc::clone (atomic bump, cheap). Value::Bytes clone can be expensive for large payloads. |
| `bv.reservoir_sample` | v0 / buffer | 14 ns | 35 ns | Algorithm R: deterministic xorshift PRNG, one modulo, one Value clone. Clone path variance (same as most_recent_n). |
| `bv.distance_from_home` | v0 / geo | 12 ns (update) | 32 ns (update) | UPDATE: write to ring buffer at head index -- O(1). QUERY: iterates ring for centroid (O(samples), samples<=100, cold path). Update cost is Tier 1 floor; query cost may dominate in query-heavy pipelines. |
| `bv.geo_spread` | v0 / geo | 18 ns | 40 ns | Welford 2D (Welford 1962). Was O(n-entity-history) pre-fix (5,000-25,000 ns TRACED); now pure scalar. Borderline Tier 2/3 per audit line 15; kept in Tier 3 per audit classification. |

---

## Summary counts

| Tier | Ops | Floor range | Post-19.2 expected |
|---|---|---|---|
| Tier 1 (fast) | 38 ops | 4-22 ns | 25-42 ns |
| Tier 2 (moderate) | 6 ops | 18-80 ns | 35-130 ns |
| Tier 3 (algorithmic) | 9 ops | 10-250 ns | 30-300 ns |
| **Total** | **53 ops** | -- | -- |

**Tier-count derivation:** Pre-removal per `.planning/research/operator-update-uniformity-audit.md`
lines 13-15 + 177: Tier 1 = 38, Tier 2 = 8, Tier 3 = 9, Total = 55. `bv.unique_cells` and
`bv.geo_entropy` were both Tier 2 (audit lines 173-174 + 206: "BTreeMap of grid cells;
~40-70 ns"). a later change removed both. Post-removal: **38 Tier 1 + 6 Tier 2 + 9 Tier 3 = 53**.

---

## Recipe Replacements (v0)

| Removed op | Replacement recipe | Tier | Notes |
|---|---|---|---|
| `bv.unique_cells` | `bv.n_unique(quadkey(lat, lon, zoom))` | 2 (HLL post-promotion) | Bounded memory; +-1.6% error at HLL threshold. Use `n_unique` for cardinality; the quadkey expression computes a deterministic integer cell id at apply time. (Op was renamed from `count_distinct` to `n_unique` in ADR-002, v0-01.) |
| `bv.geo_entropy` | `bv.entropy(quadkey(lat, lon, zoom))` | 3 (BTreeMap + key insert) | Default `max_categories=1024`. For high-mobility entities consider a lower cap to bound per-entity memory. |

Both replacements are direct drop-ins: the `quadkey(lat, lon, zoom)` expression (v0
expression DSL) produces the same deterministic quadtile key that `unique_cells` and
`geo_entropy` computed internally.

---

## How this catalogue stays fresh

Hand-maintained per D-06. Source of truth:
`.planning/research/operator-update-uniformity-audit.md` + `.planning/perf-baselines.md`.

When a future phase changes per-op cost (algorithm replacement, new sketch, wrapping-fix
landing):

1. Update the op's row in this file in the same PR as the implementation.
2. Update the "Post-19.2 expected" column if the baseline has shifted.
3. If the tier changes (e.g., a Tier 3 op moves to Tier 2 after an algorithm swap), move
   its row to the new section.

Reviewer discipline is the staleness mitigation. If drift becomes a problem post-v0, the
upgrade path is source-attribute via proc-macro (see CONTEXT.md Deferred Ideas:
`#[doc(cost = "tier1")]` annotation + build-script extraction). v0 stays markdown.
