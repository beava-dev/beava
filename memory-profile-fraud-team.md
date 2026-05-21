# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `fraud`
- Events requested from generator: `2000`
- Events replayed from generator: `2000`
- Events by source:
  - `Txn`: `2000`
- Derivations discovered: `9`
- Aggregate features discovered: `111`
- Active entity rows profiled: `5422`
- Bytes per active entity row p99: `26655` bytes

## Per-Entity Table Footprint

| Rank | Table | Source | group_by key | Active entities | Features/entity | Events applied | Stack p50 | Stack p99 | Stack max | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max | Top contributor |
|------|-------|--------|--------------|-----------------|-----------------|----------------|-----------|-----------|-----------|----------|----------|----------|-----------|-----------|-----------|-----------------|
| 1 | `TxnByUser` | `Txn` | `user_id` | 1982 | 62 | 2000 | 4960 | 4960 | 4960 | 21689 | 21697 | 22013 | 26649 | 26657 | 26973 | `amount_p95_24h` |
| 2 | `TxnByMerchant` | `Txn` | `merchant_id` | 863 | 4 | 2000 | 320 | 320 | 320 | 3096 | 3096 | 3096 | 3416 | 3416 | 3416 | `merchant_amount_p99_24h` |
| 3 | `TxnByIp` | `Txn` | `ip_address` | 862 | 8 | 2000 | 640 | 640 | 640 | 2224 | 2608 | 2896 | 2864 | 3248 | 3536 | `ip_top_users` |
| 4 | `TxnByCard` | `Txn` | `card_fp` | 854 | 8 | 2000 | 640 | 640 | 640 | 2216 | 2216 | 2216 | 2856 | 2856 | 2856 | `small_amt_burst_5m` |
| 5 | `TxnByDevice` | `Txn` | `device_id` | 861 | 6 | 2000 | 480 | 480 | 480 | 1104 | 1104 | 1104 | 1584 | 1584 | 1584 | `cards_per_device_24h` |
| 6 | `CardAddByDevice` | `CardAdd` | `device_id` | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 7 | `LoginByUser` | `Login` | `user_id` | 0 | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 8 | `RefundByUser` | `Refund` | `user_id` | 0 | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 9 | `SignupByIp` | `Signup` | `ip_address` | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |

## Per-Table Entity Details

### `TxnByUser` (`Txn` by `user_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `amount_p95_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `p50_amount_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `p99_amount_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `dist_from_home` | `distance_from_home` | `lifetime` | 80 | 1720 | 1720 | 1720 | 1800 | 1800 | 1800 |
| `reservoir_50` | `reservoir_sample` | `lifetime` | 80 | 1600 | 1600 | 1600 | 1680 | 1680 | 1680 |
| `seasonal_dev` | `seasonal_deviation` | `lifetime` | 80 | 1424 | 1427 | 1427 | 1504 | 1507 | 1507 |
| `dow_hour_hist_30d` | `dow_hour_histogram` | `lifetime` | 80 | 1344 | 1344 | 1344 | 1424 | 1424 | 1424 |
| `device_seen` | `bloom_member` | `lifetime` | 80 | 1280 | 1280 | 1280 | 1360 | 1360 | 1360 |
| `burst_count_5m` | `burst_count` | `windowed` | 80 | 1024 | 1024 | 1024 | 1104 | 1104 | 1104 |
| `top_merchants_24h` | `top_k` | `windowed` | 80 | 512 | 512 | 608 | 592 | 592 | 688 |
| `countries_distinct_7d` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `ips_distinct_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `merchants_distinct_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `mcc_entropy_24h` | `entropy` | `windowed` | 80 | 396 | 396 | 480 | 476 | 476 | 560 |
| `avg_amount_24h` | `mean` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `min_amount_24h` | `min` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `std_amount_24h` | `std` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `sum_amount_24h` | `sum` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_count_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_count_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_count_5m` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `var_amount_24h` | `var` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `hour_hist_30d` | `hour_of_day_histogram` | `lifetime` | 80 | 252 | 252 | 252 | 332 | 332 | 332 |
| `geo_spread_24h` | `geo_spread` | `lifetime` | 80 | 215 | 217 | 246 | 295 | 297 | 326 |
| `event_mix_24h` | `event_type_mix` | `lifetime` | 80 | 208 | 208 | 288 | 288 | 288 | 368 |
| `geo_kmh` | `geo_velocity` | `lifetime` | 80 | 192 | 194 | 209 | 272 | 274 | 289 |
| `geo_dist_last` | `geo_distance` | `lifetime` | 80 | 177 | 179 | 193 | 257 | 259 | 273 |
| `unique_cells_24h` | `n_unique` | `lifetime` | 80 | 168 | 168 | 168 | 248 | 248 | 248 |
| `last_5_amounts` | `last_n` | `lifetime` | 80 | 160 | 160 | 160 | 240 | 240 | 240 |
| `recent_5_amts` | `most_recent_n` | `lifetime` | 80 | 160 | 160 | 160 | 240 | 240 | 240 |
| `first_5_merchants` | `first_n` | `lifetime` | 80 | 128 | 128 | 128 | 208 | 208 | 208 |
| `amount_lag1` | `lag` | `lifetime` | 80 | 64 | 64 | 64 | 144 | 144 | 144 |
| `geo_entropy_24h` | `entropy` | `lifetime` | 80 | 56 | 56 | 56 | 136 | 136 | 136 |
| `time_since_last_5` | `time_since_last_n` | `lifetime` | 80 | 40 | 40 | 40 | 120 | 120 | 120 |
| `age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_decayed_sum_24h` | `decayed_sum` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_delta` | `delta_from_prev` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_ew_zscore` | `ew_zscore` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_ewma_1h` | `ewma` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_ewvar_1h` | `ewvar` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_outliers_5m` | `outlier_count` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_rate_5m` | `rate_of_change` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_trend_5m` | `trend` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_trend_resid_5m` | `trend_residual` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_twa_5m` | `twa` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `amount_z_score` | `z_score` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `decline_streak` | `negative_streak` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_change_count_5m` | `value_change_count` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `first_amount` | `first` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `first_in_24h` | `first_seen_in_window` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `has_seen` | `has_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `inter_arrival_1h` | `inter_arrival_stats` | `windowed` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `last_amount` | `last` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `last_seen` | `last_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `max_amount_lifetime` | `max` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `max_streak` | `max_streak` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `sum_amount_lifetime` | `sum` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `time_since_last` | `time_since` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `txn_count_lifetime` | `count` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `txn_decayed_count_24h` | `decayed_count` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `txn_streak` | `streak` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `k00029943` | 2 | 4960 | 22013 | 26973 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00055008` | 2 | 4960 | 22013 | 26973 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00067574` | 2 | 4960 | 22010 | 26970 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00070333` | 2 | 4960 | 22009 | 26969 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00022821` | 2 | 4960 | 22008 | 26968 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |

#### Feature Breakdown For Largest Entity `k00029943`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `amount_p95_24h` | `quantile` | `windowed` | 2 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `p50_amount_24h` | `quantile` | `windowed` | 2 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `p99_amount_24h` | `quantile` | `windowed` | 2 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `dist_from_home` | `distance_from_home` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 |
| `reservoir_50` | `reservoir_sample` | `lifetime` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 |
| `seasonal_dev` | `seasonal_deviation` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 1426 | 1506 |
| `dow_hour_hist_30d` | `dow_hour_histogram` | `lifetime` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 |
| `device_seen` | `bloom_member` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 |
| `burst_count_5m` | `burst_count` | `windowed` | 2 | 80 | 80 | 72 | 8 | 1024 | 1104 |
| `top_merchants_24h` | `top_k` | `windowed` | 2 | 80 | 80 | 8 | 72 | 608 | 688 |
| `mcc_entropy_24h` | `entropy` | `windowed` | 2 | 80 | 80 | 8 | 72 | 480 | 560 |
| `countries_distinct_7d` | `n_unique` | `windowed` | 2 | 80 | 80 | 8 | 72 | 424 | 504 |
| `ips_distinct_24h` | `n_unique` | `windowed` | 2 | 80 | 80 | 8 | 72 | 424 | 504 |
| `merchants_distinct_24h` | `n_unique` | `windowed` | 2 | 80 | 80 | 8 | 72 | 424 | 504 |
| `event_mix_24h` | `event_type_mix` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 288 | 368 |
| `avg_amount_24h` | `mean` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `min_amount_24h` | `min` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `std_amount_24h` | `std` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `sum_amount_24h` | `sum` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_count_1h` | `count` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_count_24h` | `count` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_count_5m` | `count` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `var_amount_24h` | `var` | `windowed` | 2 | 80 | 80 | 8 | 72 | 256 | 336 |
| `hour_hist_30d` | `hour_of_day_histogram` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 252 | 332 |
| `geo_spread_24h` | `geo_spread` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 246 | 326 |
| `geo_kmh` | `geo_velocity` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 208 | 288 |
| `geo_dist_last` | `geo_distance` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 193 | 273 |
| `unique_cells_24h` | `n_unique` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 168 | 248 |
| `last_5_amounts` | `last_n` | `lifetime` | 2 | 80 | 80 | 40 | 40 | 160 | 240 |
| `recent_5_amts` | `most_recent_n` | `lifetime` | 2 | 80 | 80 | 48 | 32 | 160 | 240 |
| `first_5_merchants` | `first_n` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 128 | 208 |
| `amount_lag1` | `lag` | `lifetime` | 2 | 80 | 80 | 40 | 40 | 64 | 144 |
| `geo_entropy_24h` | `entropy` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 56 | 136 |
| `time_since_last_5` | `time_since_last_n` | `lifetime` | 2 | 80 | 80 | 40 | 40 | 40 | 120 |
| `age` | `age` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_decayed_sum_24h` | `decayed_sum` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_delta` | `delta_from_prev` | `lifetime` | 2 | 80 | 80 | 24 | 56 | 0 | 80 |
| `amount_ew_zscore` | `ew_zscore` | `lifetime` | 2 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_ewma_1h` | `ewma` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_ewvar_1h` | `ewvar` | `lifetime` | 2 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_outliers_5m` | `outlier_count` | `windowed` | 2 | 80 | 80 | 40 | 40 | 0 | 80 |
| `amount_rate_5m` | `rate_of_change` | `windowed` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_trend_5m` | `trend` | `windowed` | 2 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_trend_resid_5m` | `trend_residual` | `windowed` | 2 | 80 | 80 | 72 | 8 | 0 | 80 |
| `amount_twa_5m` | `twa` | `windowed` | 2 | 80 | 80 | 40 | 40 | 0 | 80 |
| `amount_z_score` | `z_score` | `windowed` | 2 | 80 | 80 | 40 | 40 | 0 | 80 |
| `decline_streak` | `negative_streak` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 0 | 80 |
| `device_change_count_5m` | `value_change_count` | `windowed` | 2 | 80 | 80 | 40 | 40 | 0 | 80 |
| `first_amount` | `first` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `first_in_24h` | `first_seen_in_window` | `windowed` | 2 | 80 | 80 | 24 | 56 | 0 | 80 |
| `first_seen` | `first_seen` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `has_seen` | `has_seen` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `inter_arrival_1h` | `inter_arrival_stats` | `windowed` | 2 | 80 | 80 | 40 | 40 | 0 | 80 |
| `last_amount` | `last` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `last_seen` | `last_seen` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `max_amount_lifetime` | `max` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `max_streak` | `max_streak` | `lifetime` | 2 | 80 | 80 | 16 | 64 | 0 | 80 |
| `sum_amount_lifetime` | `sum` | `lifetime` | 2 | 80 | 80 | 16 | 64 | 0 | 80 |
| `time_since_last` | `time_since` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `txn_count_lifetime` | `count` | `lifetime` | 2 | 80 | 80 | 8 | 72 | 0 | 80 |
| `txn_decayed_count_24h` | `decayed_count` | `lifetime` | 2 | 80 | 80 | 32 | 48 | 0 | 80 |
| `txn_streak` | `streak` | `lifetime` | 2 | 80 | 80 | 16 | 64 | 0 | 80 |

### `TxnByMerchant` (`Txn` by `merchant_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `merchant_amount_p99_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `users_per_merchant_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `txn_per_merchant_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `merchant_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s470` | 9 | 320 | 3096 | 3416 | `merchant_amount_p99_24h`=2496 bytes, `users_per_merchant_24h`=504 bytes, `txn_per_merchant_24h`=336 bytes |
| `s176` | 7 | 320 | 3096 | 3416 | `merchant_amount_p99_24h`=2496 bytes, `users_per_merchant_24h`=504 bytes, `txn_per_merchant_24h`=336 bytes |
| `s278` | 7 | 320 | 3096 | 3416 | `merchant_amount_p99_24h`=2496 bytes, `users_per_merchant_24h`=504 bytes, `txn_per_merchant_24h`=336 bytes |
| `s387` | 7 | 320 | 3096 | 3416 | `merchant_amount_p99_24h`=2496 bytes, `users_per_merchant_24h`=504 bytes, `txn_per_merchant_24h`=336 bytes |
| `s507` | 7 | 320 | 3096 | 3416 | `merchant_amount_p99_24h`=2496 bytes, `users_per_merchant_24h`=504 bytes, `txn_per_merchant_24h`=336 bytes |

#### Feature Breakdown For Largest Entity `s470`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `merchant_amount_p99_24h` | `quantile` | `windowed` | 9 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `users_per_merchant_24h` | `n_unique` | `windowed` | 9 | 80 | 80 | 8 | 72 | 424 | 504 |
| `txn_per_merchant_24h` | `count` | `windowed` | 9 | 80 | 80 | 8 | 72 | 256 | 336 |
| `merchant_first_seen` | `first_seen` | `lifetime` | 9 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByIp` (`Txn` by `ip_address`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `ip_top_users` | `top_k` | `windowed` | 80 | 608 | 992 | 1280 | 688 | 1072 | 1360 |
| `cards_per_ip_1h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `users_per_ip_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `amount_sum_per_ip_1h` | `sum` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_ip_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_ip_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `ip_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `ip_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s389` | 9 | 640 | 2896 | 3536 | `ip_top_users`=1360 bytes, `cards_per_ip_1h`=504 bytes, `users_per_ip_24h`=504 bytes |
| `s122` | 8 | 640 | 2800 | 3440 | `ip_top_users`=1264 bytes, `cards_per_ip_1h`=504 bytes, `users_per_ip_24h`=504 bytes |
| `s466` | 8 | 640 | 2800 | 3440 | `ip_top_users`=1264 bytes, `cards_per_ip_1h`=504 bytes, `users_per_ip_24h`=504 bytes |
| `s132` | 7 | 640 | 2704 | 3344 | `ip_top_users`=1168 bytes, `cards_per_ip_1h`=504 bytes, `users_per_ip_24h`=504 bytes |
| `s226` | 7 | 640 | 2704 | 3344 | `ip_top_users`=1168 bytes, `cards_per_ip_1h`=504 bytes, `users_per_ip_24h`=504 bytes |

#### Feature Breakdown For Largest Entity `s389`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `ip_top_users` | `top_k` | `windowed` | 9 | 80 | 80 | 8 | 72 | 1280 | 1360 |
| `cards_per_ip_1h` | `n_unique` | `windowed` | 9 | 80 | 80 | 8 | 72 | 424 | 504 |
| `users_per_ip_24h` | `n_unique` | `windowed` | 9 | 80 | 80 | 8 | 72 | 424 | 504 |
| `amount_sum_per_ip_1h` | `sum` | `windowed` | 9 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_ip_1h` | `count` | `windowed` | 9 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_ip_24h` | `count` | `windowed` | 9 | 80 | 80 | 8 | 72 | 256 | 336 |
| `ip_age` | `age` | `lifetime` | 9 | 80 | 80 | 32 | 48 | 0 | 80 |
| `ip_first_seen` | `first_seen` | `lifetime` | 9 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByCard` (`Txn` by `card_fp`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `small_amt_burst_5m` | `burst_count` | `windowed` | 80 | 1024 | 1024 | 1024 | 1104 | 1104 | 1104 |
| `merchants_per_card_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `decline_count_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_card_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_card_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `card_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `card_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `decline_streak_card` | `negative_streak` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s409` | 10 | 640 | 2216 | 2856 | `small_amt_burst_5m`=1104 bytes, `merchants_per_card_24h`=504 bytes, `decline_count_1h`=336 bytes |
| `s179` | 7 | 640 | 2216 | 2856 | `small_amt_burst_5m`=1104 bytes, `merchants_per_card_24h`=504 bytes, `decline_count_1h`=336 bytes |
| `s42` | 7 | 640 | 2216 | 2856 | `small_amt_burst_5m`=1104 bytes, `merchants_per_card_24h`=504 bytes, `decline_count_1h`=336 bytes |
| `s450` | 7 | 640 | 2216 | 2856 | `small_amt_burst_5m`=1104 bytes, `merchants_per_card_24h`=504 bytes, `decline_count_1h`=336 bytes |
| `s896` | 7 | 640 | 2216 | 2856 | `small_amt_burst_5m`=1104 bytes, `merchants_per_card_24h`=504 bytes, `decline_count_1h`=336 bytes |

#### Feature Breakdown For Largest Entity `s409`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `small_amt_burst_5m` | `burst_count` | `windowed` | 10 | 80 | 80 | 72 | 8 | 1024 | 1104 |
| `merchants_per_card_24h` | `n_unique` | `windowed` | 10 | 80 | 80 | 8 | 72 | 424 | 504 |
| `decline_count_1h` | `count` | `windowed` | 10 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_card_1h` | `count` | `windowed` | 10 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_card_24h` | `count` | `windowed` | 10 | 80 | 80 | 8 | 72 | 256 | 336 |
| `card_age` | `age` | `lifetime` | 10 | 80 | 80 | 32 | 48 | 0 | 80 |
| `card_first_seen` | `first_seen` | `lifetime` | 10 | 80 | 80 | 32 | 48 | 0 | 80 |
| `decline_streak_card` | `negative_streak` | `lifetime` | 10 | 80 | 80 | 8 | 72 | 0 | 80 |

### `TxnByDevice` (`Txn` by `device_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `cards_per_device_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `users_per_device_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `device_txn_count_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `device_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_last_seen` | `last_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s377` | 7 | 480 | 1104 | 1584 | `cards_per_device_24h`=504 bytes, `users_per_device_24h`=504 bytes, `device_txn_count_24h`=336 bytes |
| `s717` | 7 | 480 | 1104 | 1584 | `cards_per_device_24h`=504 bytes, `users_per_device_24h`=504 bytes, `device_txn_count_24h`=336 bytes |
| `s743` | 7 | 480 | 1104 | 1584 | `cards_per_device_24h`=504 bytes, `users_per_device_24h`=504 bytes, `device_txn_count_24h`=336 bytes |
| `s107` | 6 | 480 | 1104 | 1584 | `cards_per_device_24h`=504 bytes, `users_per_device_24h`=504 bytes, `device_txn_count_24h`=336 bytes |
| `s171` | 6 | 480 | 1104 | 1584 | `cards_per_device_24h`=504 bytes, `users_per_device_24h`=504 bytes, `device_txn_count_24h`=336 bytes |

#### Feature Breakdown For Largest Entity `s377`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `cards_per_device_24h` | `n_unique` | `windowed` | 7 | 80 | 80 | 8 | 72 | 424 | 504 |
| `users_per_device_24h` | `n_unique` | `windowed` | 7 | 80 | 80 | 8 | 72 | 424 | 504 |
| `device_txn_count_24h` | `count` | `windowed` | 7 | 80 | 80 | 8 | 72 | 256 | 336 |
| `device_age` | `age` | `lifetime` | 7 | 80 | 80 | 32 | 48 | 0 | 80 |
| `device_first_seen` | `first_seen` | `lifetime` | 7 | 80 | 80 | 32 | 48 | 0 | 80 |
| `device_last_seen` | `last_seen` | `lifetime` | 7 | 80 | 80 | 32 | 48 | 0 | 80 |

### `CardAddByDevice` (`CardAdd` by `device_id`)

No active entity rows. Configured features: `3`. The workload generator emitted no events for this table's source.

### `LoginByUser` (`Login` by `user_id`)

No active entity rows. Configured features: `8`. The workload generator emitted no events for this table's source.

### `RefundByUser` (`Refund` by `user_id`)

No active entity rows. Configured features: `8`. The workload generator emitted no events for this table's source.

### `SignupByIp` (`Signup` by `ip_address`)

No active entity rows. Configured features: `4`. The workload generator emitted no events for this table's source.

## Sorted Op Table

| Rank | Op | Shape | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|------|----|-------|-------------|-----------------|---------------|-------------|------------|-------------|
| 1 | `quantile` | `windowed` | 544720 | 544720 | 54472 | 490248 | 16450544 | 16995264 |
| 2 | `n_unique` | `windowed` | 888720 | 888720 | 88872 | 799848 | 4710216 | 5598936 |
| 3 | `count` | `windowed` | 956480 | 956480 | 95648 | 860832 | 3060736 | 4017216 |
| 4 | `distance_from_home` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 3409040 | 3567600 |
| 5 | `reservoir_sample` | `lifetime` | 158560 | 158560 | 79280 | 79280 | 3171200 | 3329760 |
| 6 | `burst_count` | `windowed` | 226880 | 226880 | 204192 | 22688 | 2904064 | 3130944 |
| 7 | `seasonal_deviation` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 2823558 | 2982118 |
| 8 | `dow_hour_histogram` | `lifetime` | 158560 | 158560 | 47568 | 110992 | 2663808 | 2822368 |
| 9 | `bloom_member` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 2536960 | 2695520 |
| 10 | `top_k` | `windowed` | 227520 | 227520 | 22752 | 204768 | 1567104 | 1794624 |
| 11 | `sum` | `windowed` | 227520 | 227520 | 22752 | 204768 | 728064 | 955584 |
| 12 | `entropy` | `windowed` | 158560 | 158560 | 15856 | 142704 | 786164 | 944724 |
| 13 | `mean` | `windowed` | 158560 | 158560 | 15856 | 142704 | 507392 | 665952 |
| 14 | `min` | `windowed` | 158560 | 158560 | 15856 | 142704 | 507392 | 665952 |
| 15 | `std` | `windowed` | 158560 | 158560 | 15856 | 142704 | 507392 | 665952 |
| 16 | `var` | `windowed` | 158560 | 158560 | 15856 | 142704 | 507392 | 665952 |
| 17 | `hour_of_day_histogram` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 499464 | 658024 |
| 18 | `geo_spread` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 427321 | 585881 |
| 19 | `event_type_mix` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 413696 | 572256 |
| 20 | `geo_velocity` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 381469 | 540029 |
| 21 | `geo_distance` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 351735 | 510295 |
| 22 | `n_unique` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 332976 | 491536 |
| 23 | `last_n` | `lifetime` | 158560 | 158560 | 79280 | 79280 | 317120 | 475680 |
| 24 | `most_recent_n` | `lifetime` | 158560 | 158560 | 95136 | 63424 | 317120 | 475680 |
| 25 | `first_seen` | `lifetime` | 433760 | 433760 | 173504 | 260256 | 0 | 433760 |
| 26 | `first_n` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 253696 | 412256 |
| 27 | `age` | `lifetime` | 364720 | 364720 | 145888 | 218832 | 0 | 364720 |
| 28 | `lag` | `lifetime` | 158560 | 158560 | 79280 | 79280 | 126848 | 285408 |
| 29 | `entropy` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 110992 | 269552 |
| 30 | `time_since_last_n` | `lifetime` | 158560 | 158560 | 79280 | 79280 | 79280 | 237840 |
| 31 | `last_seen` | `lifetime` | 227440 | 227440 | 90976 | 136464 | 0 | 227440 |
| 32 | `negative_streak` | `lifetime` | 226880 | 226880 | 22688 | 204192 | 0 | 226880 |
| 33 | `count` | `lifetime` | 158560 | 158560 | 15856 | 142704 | 0 | 158560 |
| 34 | `decayed_count` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 35 | `decayed_sum` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 36 | `delta_from_prev` | `lifetime` | 158560 | 158560 | 47568 | 110992 | 0 | 158560 |
| 37 | `ew_zscore` | `lifetime` | 158560 | 158560 | 95136 | 63424 | 0 | 158560 |
| 38 | `ewma` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 39 | `ewvar` | `lifetime` | 158560 | 158560 | 95136 | 63424 | 0 | 158560 |
| 40 | `first` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 41 | `first_seen_in_window` | `windowed` | 158560 | 158560 | 47568 | 110992 | 0 | 158560 |
| 42 | `has_seen` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 43 | `inter_arrival_stats` | `windowed` | 158560 | 158560 | 79280 | 79280 | 0 | 158560 |
| 44 | `last` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 45 | `max` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 46 | `max_streak` | `lifetime` | 158560 | 158560 | 31712 | 126848 | 0 | 158560 |
| 47 | `outlier_count` | `windowed` | 158560 | 158560 | 79280 | 79280 | 0 | 158560 |
| 48 | `rate_of_change` | `windowed` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 49 | `streak` | `lifetime` | 158560 | 158560 | 31712 | 126848 | 0 | 158560 |
| 50 | `sum` | `lifetime` | 158560 | 158560 | 31712 | 126848 | 0 | 158560 |
| 51 | `time_since` | `lifetime` | 158560 | 158560 | 63424 | 95136 | 0 | 158560 |
| 52 | `trend` | `windowed` | 158560 | 158560 | 95136 | 63424 | 0 | 158560 |
| 53 | `trend_residual` | `windowed` | 158560 | 158560 | 142704 | 15856 | 0 | 158560 |
| 54 | `twa` | `windowed` | 158560 | 158560 | 79280 | 79280 | 0 | 158560 |
| 55 | `value_change_count` | `windowed` | 158560 | 158560 | 79280 | 79280 | 0 | 158560 |
| 56 | `z_score` | `windowed` | 158560 | 158560 | 79280 | 79280 | 0 | 158560 |

## Sorted Op Entity-Feature Details

Secondary diagnostic view: top entity-feature contributors under each op/shape parent. The table/entity sections above are the primary bytes-per-entity view.

### 1. `quantile` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 1 | `Txn` | `TxnByMerchant` | `s470` | `merchant_amount_p99_24h` | `merchant_id` | 9 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s176` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s278` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s387` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s507` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s570` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s707` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s825` | `merchant_amount_p99_24h` | `merchant_id` | 7 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s12` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s172` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s201` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s365` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s371` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s39` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s498` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s537` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s591` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s643` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s655` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |
| 1 | `Txn` | `TxnByMerchant` | `s717` | `merchant_amount_p99_24h` | `merchant_id` | 6 | 80 | 80 | 8 | 72 | 2416 | 2496 | 0.0% |

Showing top 20 of `6809` entity-feature rows for this op/shape.

### 2. `n_unique` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 2 | `Txn` | `TxnByCard` | `s409` | `merchants_per_card_24h` | `card_fp` | 10 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s389` | `cards_per_ip_1h` | `ip_address` | 9 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s389` | `users_per_ip_24h` | `ip_address` | 9 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByMerchant` | `s470` | `users_per_merchant_24h` | `merchant_id` | 9 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s122` | `cards_per_ip_1h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s122` | `users_per_ip_24h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s466` | `cards_per_ip_1h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s466` | `users_per_ip_24h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByCard` | `s179` | `merchants_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByCard` | `s42` | `merchants_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByCard` | `s450` | `merchants_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByCard` | `s896` | `merchants_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByDevice` | `s377` | `cards_per_device_24h` | `device_id` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByDevice` | `s377` | `users_per_device_24h` | `device_id` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByDevice` | `s717` | `cards_per_device_24h` | `device_id` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByDevice` | `s717` | `users_per_device_24h` | `device_id` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByDevice` | `s743` | `cards_per_device_24h` | `device_id` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByDevice` | `s743` | `users_per_device_24h` | `device_id` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s132` | `cards_per_ip_1h` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |
| 2 | `Txn` | `TxnByIp` | `s132` | `users_per_ip_24h` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 424 | 504 | 0.0% |

Showing top 20 of `11109` entity-feature rows for this op/shape.

### 3. `count` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 3 | `Txn` | `TxnByCard` | `s409` | `decline_count_1h` | `card_fp` | 10 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s409` | `txn_per_card_1h` | `card_fp` | 10 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s409` | `txn_per_card_24h` | `card_fp` | 10 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByIp` | `s389` | `txn_per_ip_1h` | `ip_address` | 9 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByIp` | `s389` | `txn_per_ip_24h` | `ip_address` | 9 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByMerchant` | `s470` | `txn_per_merchant_24h` | `merchant_id` | 9 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByIp` | `s122` | `txn_per_ip_1h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByIp` | `s122` | `txn_per_ip_24h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByIp` | `s466` | `txn_per_ip_1h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByIp` | `s466` | `txn_per_ip_24h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s179` | `decline_count_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s179` | `txn_per_card_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s179` | `txn_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s42` | `decline_count_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s42` | `txn_per_card_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s42` | `txn_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s450` | `decline_count_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s450` | `txn_per_card_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s450` | `txn_per_card_24h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 3 | `Txn` | `TxnByCard` | `s896` | `decline_count_1h` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |

Showing top 20 of `11956` entity-feature rows for this op/shape.

### 4. `distance_from_home` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 4 | `Txn` | `TxnByUser` | `k00013835` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00017893` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00022821` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00029943` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00030706` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00030832` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00031894` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00039938` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00041244` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00041440` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00042338` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00046226` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00053345` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00055008` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00064964` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00067574` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00070333` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00089428` | `dist_from_home` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00000032` | `dist_from_home` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |
| 4 | `Txn` | `TxnByUser` | `k00000045` | `dist_from_home` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1720 | 1800 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 5. `reservoir_sample` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 5 | `Txn` | `TxnByUser` | `k00013835` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00017893` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00022821` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00029943` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00030706` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00030832` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00031894` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00039938` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00041244` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00041440` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00042338` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00046226` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00053345` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00055008` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00064964` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00067574` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00070333` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00089428` | `reservoir_50` | `user_id` | 2 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00000032` | `reservoir_50` | `user_id` | 1 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |
| 5 | `Txn` | `TxnByUser` | `k00000045` | `reservoir_50` | `user_id` | 1 | 80 | 80 | 40 | 40 | 1600 | 1680 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 6. `burst_count` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 6 | `Txn` | `TxnByCard` | `s409` | `small_amt_burst_5m` | `card_fp` | 10 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s179` | `small_amt_burst_5m` | `card_fp` | 7 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s42` | `small_amt_burst_5m` | `card_fp` | 7 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s450` | `small_amt_burst_5m` | `card_fp` | 7 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s896` | `small_amt_burst_5m` | `card_fp` | 7 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s138` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s274` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s337` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s354` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s433` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s474` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s555` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s560` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s566` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s654` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s770` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s957` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s996` | `small_amt_burst_5m` | `card_fp` | 6 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s129` | `small_amt_burst_5m` | `card_fp` | 5 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |
| 6 | `Txn` | `TxnByCard` | `s150` | `small_amt_burst_5m` | `card_fp` | 5 | 80 | 80 | 72 | 8 | 1024 | 1104 | 0.0% |

Showing top 20 of `2836` entity-feature rows for this op/shape.

### 7. `seasonal_deviation` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 7 | `Txn` | `TxnByUser` | `k00000181` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00000870` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00001150` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00001671` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00001874` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00001952` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00002108` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00002920` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00003254` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00003623` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00004116` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00004151` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00004200` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00004653` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00005051` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00005603` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00006199` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00006234` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00006914` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |
| 7 | `Txn` | `TxnByUser` | `k00007171` | `seasonal_dev` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1427 | 1507 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 8. `dow_hour_histogram` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 8 | `Txn` | `TxnByUser` | `k00013835` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00017893` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00022821` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00029943` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00030706` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00030832` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00031894` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00039938` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00041244` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00041440` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00042338` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00046226` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00053345` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00055008` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00064964` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00067574` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00070333` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00089428` | `dow_hour_hist_30d` | `user_id` | 2 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00000032` | `dow_hour_hist_30d` | `user_id` | 1 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |
| 8 | `Txn` | `TxnByUser` | `k00000045` | `dow_hour_hist_30d` | `user_id` | 1 | 80 | 80 | 24 | 56 | 1344 | 1424 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 9. `bloom_member` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 9 | `Txn` | `TxnByUser` | `k00013835` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00017893` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00022821` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00029943` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00030706` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00030832` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00031894` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00039938` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00041244` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00041440` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00042338` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00046226` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00053345` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00055008` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00064964` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00067574` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00070333` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00089428` | `device_seen` | `user_id` | 2 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00000032` | `device_seen` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 9 | `Txn` | `TxnByUser` | `k00000045` | `device_seen` | `user_id` | 1 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 10. `top_k` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 10 | `Txn` | `TxnByIp` | `s389` | `ip_top_users` | `ip_address` | 9 | 80 | 80 | 8 | 72 | 1280 | 1360 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s122` | `ip_top_users` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 1184 | 1264 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s466` | `ip_top_users` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 1184 | 1264 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s132` | `ip_top_users` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 1088 | 1168 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s226` | `ip_top_users` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 1088 | 1168 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s683` | `ip_top_users` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 1088 | 1168 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s789` | `ip_top_users` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 1088 | 1168 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s108` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s140` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s161` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s225` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s227` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s279` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s486` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s629` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s681` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s69` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s780` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s825` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |
| 10 | `Txn` | `TxnByIp` | `s928` | `ip_top_users` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 992 | 1072 | 0.1% |

Showing top 20 of `2844` entity-feature rows for this op/shape.

### 11. `sum` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 11 | `Txn` | `TxnByIp` | `s389` | `amount_sum_per_ip_1h` | `ip_address` | 9 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s122` | `amount_sum_per_ip_1h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s466` | `amount_sum_per_ip_1h` | `ip_address` | 8 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s132` | `amount_sum_per_ip_1h` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s226` | `amount_sum_per_ip_1h` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s683` | `amount_sum_per_ip_1h` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s789` | `amount_sum_per_ip_1h` | `ip_address` | 7 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s108` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s140` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s161` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s225` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s227` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s279` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s486` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s629` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s681` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s69` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s780` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s825` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |
| 11 | `Txn` | `TxnByIp` | `s928` | `amount_sum_per_ip_1h` | `ip_address` | 6 | 80 | 80 | 8 | 72 | 256 | 336 | 0.0% |

Showing top 20 of `2844` entity-feature rows for this op/shape.

### 12. `entropy` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 12 | `Txn` | `TxnByUser` | `k00013835` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00017893` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00022821` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00029943` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00030706` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00030832` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00031894` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00041244` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00042338` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00046226` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00053345` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00055008` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00064964` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00067574` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00070333` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00089428` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 480 | 560 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00039938` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 479 | 559 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00041440` | `mcc_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 479 | 559 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00000032` | `mcc_entropy_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 396 | 476 | 0.1% |
| 12 | `Txn` | `TxnByUser` | `k00000045` | `mcc_entropy_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 396 | 476 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 13. `mean` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 13 | `Txn` | `TxnByUser` | `k00013835` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00017893` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00022821` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00029943` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00030706` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00030832` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00031894` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00039938` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00041244` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00041440` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00042338` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00046226` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00053345` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00055008` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00064964` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00067574` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00070333` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00089428` | `avg_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00000032` | `avg_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 13 | `Txn` | `TxnByUser` | `k00000045` | `avg_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 14. `min` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 14 | `Txn` | `TxnByUser` | `k00013835` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00017893` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00022821` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00029943` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00030706` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00030832` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00031894` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00039938` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00041244` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00041440` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00042338` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00046226` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00053345` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00055008` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00064964` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00067574` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00070333` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00089428` | `min_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00000032` | `min_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 14 | `Txn` | `TxnByUser` | `k00000045` | `min_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 15. `std` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 15 | `Txn` | `TxnByUser` | `k00013835` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00017893` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00022821` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00029943` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00030706` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00030832` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00031894` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00039938` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00041244` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00041440` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00042338` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00046226` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00053345` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00055008` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00064964` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00067574` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00070333` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00089428` | `std_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00000032` | `std_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 15 | `Txn` | `TxnByUser` | `k00000045` | `std_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 16. `var` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 16 | `Txn` | `TxnByUser` | `k00013835` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00017893` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00022821` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00029943` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00030706` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00030832` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00031894` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00039938` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00041244` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00041440` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00042338` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00046226` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00053345` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00055008` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00064964` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00067574` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00070333` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00089428` | `var_amount_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00000032` | `var_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |
| 16 | `Txn` | `TxnByUser` | `k00000045` | `var_amount_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 256 | 336 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 17. `hour_of_day_histogram` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 17 | `Txn` | `TxnByUser` | `k00013835` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00017893` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00022821` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00029943` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00030706` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00030832` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00031894` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00039938` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00041244` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00041440` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00042338` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00046226` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00053345` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00055008` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00064964` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00067574` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00070333` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00089428` | `hour_hist_30d` | `user_id` | 2 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00000032` | `hour_hist_30d` | `user_id` | 1 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |
| 17 | `Txn` | `TxnByUser` | `k00000045` | `hour_hist_30d` | `user_id` | 1 | 80 | 80 | 8 | 72 | 252 | 332 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 18. `geo_spread` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 18 | `Txn` | `TxnByUser` | `k00029943` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 246 | 326 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00013835` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00022821` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00041244` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00041440` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00042338` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00046226` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00055008` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00070333` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 245 | 325 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00017893` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00030706` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00031894` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00039938` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00064964` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00067574` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00089428` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 244 | 324 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00030832` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 243 | 323 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00053345` | `geo_spread_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 243 | 323 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00000113` | `geo_spread_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 217 | 297 | 0.1% |
| 18 | `Txn` | `TxnByUser` | `k00001471` | `geo_spread_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 217 | 297 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 19. `event_type_mix` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 19 | `Txn` | `TxnByUser` | `k00013835` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00017893` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00022821` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00029943` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00030706` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00030832` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00031894` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00039938` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00041244` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00041440` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00042338` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00046226` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00053345` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00055008` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00064964` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00067574` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00070333` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00089428` | `event_mix_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 288 | 368 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00000032` | `event_mix_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 208 | 288 | 0.1% |
| 19 | `Txn` | `TxnByUser` | `k00000045` | `event_mix_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 208 | 288 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 20. `geo_velocity` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 20 | `Txn` | `TxnByUser` | `k00055008` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 209 | 289 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00029943` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 208 | 288 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00067574` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 208 | 288 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00070333` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 208 | 288 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00017893` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 207 | 287 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00022821` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 207 | 287 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00030706` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 207 | 287 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00039938` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 207 | 287 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00053345` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 207 | 287 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00013835` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00030832` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00031894` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00041440` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00042338` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00046226` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00064964` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00089428` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 206 | 286 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00041244` | `geo_kmh` | `user_id` | 2 | 80 | 80 | 8 | 72 | 204 | 284 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00000113` | `geo_kmh` | `user_id` | 1 | 80 | 80 | 8 | 72 | 194 | 274 | 0.1% |
| 20 | `Txn` | `TxnByUser` | `k00001471` | `geo_kmh` | `user_id` | 1 | 80 | 80 | 8 | 72 | 194 | 274 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 21. `geo_distance` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 21 | `Txn` | `TxnByUser` | `k00029943` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 193 | 273 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00055008` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 193 | 273 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00067574` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 193 | 273 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00070333` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 193 | 273 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00030706` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 192 | 272 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00046226` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 192 | 272 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00053345` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 192 | 272 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00013835` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00017893` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00022821` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00031894` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00039938` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00041440` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00042338` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00064964` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00089428` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 191 | 271 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00030832` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 190 | 270 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00041244` | `geo_dist_last` | `user_id` | 2 | 80 | 80 | 8 | 72 | 189 | 269 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00000113` | `geo_dist_last` | `user_id` | 1 | 80 | 80 | 8 | 72 | 179 | 259 | 0.1% |
| 21 | `Txn` | `TxnByUser` | `k00001471` | `geo_dist_last` | `user_id` | 1 | 80 | 80 | 8 | 72 | 179 | 259 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 22. `n_unique` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 22 | `Txn` | `TxnByUser` | `k00013835` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00017893` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00022821` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00029943` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00030706` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00030832` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00031894` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00039938` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00041244` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00041440` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00042338` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00046226` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00053345` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00055008` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00064964` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00067574` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00070333` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00089428` | `unique_cells_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00000032` | `unique_cells_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |
| 22 | `Txn` | `TxnByUser` | `k00000045` | `unique_cells_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 168 | 248 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 23. `last_n` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 23 | `Txn` | `TxnByUser` | `k00013835` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00017893` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00022821` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00029943` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00030706` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00030832` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00031894` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00039938` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00041244` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00041440` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00042338` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00046226` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00053345` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00055008` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00064964` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00067574` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00070333` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00089428` | `last_5_amounts` | `user_id` | 2 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00000032` | `last_5_amounts` | `user_id` | 1 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |
| 23 | `Txn` | `TxnByUser` | `k00000045` | `last_5_amounts` | `user_id` | 1 | 80 | 80 | 40 | 40 | 160 | 240 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 24. `most_recent_n` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 24 | `Txn` | `TxnByUser` | `k00013835` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00017893` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00022821` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00029943` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00030706` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00030832` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00031894` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00039938` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00041244` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00041440` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00042338` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00046226` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00053345` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00055008` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00064964` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00067574` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00070333` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00089428` | `recent_5_amts` | `user_id` | 2 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00000032` | `recent_5_amts` | `user_id` | 1 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |
| 24 | `Txn` | `TxnByUser` | `k00000045` | `recent_5_amts` | `user_id` | 1 | 80 | 80 | 48 | 32 | 160 | 240 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 25. `first_seen` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 25 | `Txn` | `TxnByCard` | `s409` | `card_first_seen` | `card_fp` | 10 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s389` | `ip_first_seen` | `ip_address` | 9 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByMerchant` | `s470` | `merchant_first_seen` | `merchant_id` | 9 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s122` | `ip_first_seen` | `ip_address` | 8 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s466` | `ip_first_seen` | `ip_address` | 8 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByCard` | `s179` | `card_first_seen` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByCard` | `s42` | `card_first_seen` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByCard` | `s450` | `card_first_seen` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByCard` | `s896` | `card_first_seen` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByDevice` | `s377` | `device_first_seen` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByDevice` | `s717` | `device_first_seen` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByDevice` | `s743` | `device_first_seen` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s132` | `ip_first_seen` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s226` | `ip_first_seen` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s683` | `ip_first_seen` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByIp` | `s789` | `ip_first_seen` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByMerchant` | `s176` | `merchant_first_seen` | `merchant_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByMerchant` | `s278` | `merchant_first_seen` | `merchant_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByMerchant` | `s387` | `merchant_first_seen` | `merchant_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 25 | `Txn` | `TxnByMerchant` | `s507` | `merchant_first_seen` | `merchant_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |

Showing top 20 of `5422` entity-feature rows for this op/shape.

### 26. `first_n` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 26 | `Txn` | `TxnByUser` | `k00013835` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00017893` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00022821` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00029943` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00030706` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00030832` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00031894` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00039938` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00041244` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00041440` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00042338` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00046226` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00053345` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00055008` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00064964` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00067574` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00070333` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00089428` | `first_5_merchants` | `user_id` | 2 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00000032` | `first_5_merchants` | `user_id` | 1 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |
| 26 | `Txn` | `TxnByUser` | `k00000045` | `first_5_merchants` | `user_id` | 1 | 80 | 80 | 32 | 48 | 128 | 208 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 27. `age` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 27 | `Txn` | `TxnByCard` | `s409` | `card_age` | `card_fp` | 10 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s389` | `ip_age` | `ip_address` | 9 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s122` | `ip_age` | `ip_address` | 8 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s466` | `ip_age` | `ip_address` | 8 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s179` | `card_age` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s42` | `card_age` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s450` | `card_age` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s896` | `card_age` | `card_fp` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByDevice` | `s377` | `device_age` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByDevice` | `s717` | `device_age` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByDevice` | `s743` | `device_age` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s132` | `ip_age` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s226` | `ip_age` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s683` | `ip_age` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByIp` | `s789` | `ip_age` | `ip_address` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s138` | `card_age` | `card_fp` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s274` | `card_age` | `card_fp` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s337` | `card_age` | `card_fp` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s354` | `card_age` | `card_fp` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 27 | `Txn` | `TxnByCard` | `s433` | `card_age` | `card_fp` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |

Showing top 20 of `4559` entity-feature rows for this op/shape.

### 28. `lag` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 28 | `Txn` | `TxnByUser` | `k00013835` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00017893` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00022821` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00029943` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00030706` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00030832` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00031894` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00039938` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00041244` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00041440` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00042338` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00046226` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00053345` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00055008` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00064964` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00067574` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00070333` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00089428` | `amount_lag1` | `user_id` | 2 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00000032` | `amount_lag1` | `user_id` | 1 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |
| 28 | `Txn` | `TxnByUser` | `k00000045` | `amount_lag1` | `user_id` | 1 | 80 | 80 | 40 | 40 | 64 | 144 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 29. `entropy` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 29 | `Txn` | `TxnByUser` | `k00013835` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00017893` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00022821` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00029943` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00030706` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00030832` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00031894` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00039938` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00041244` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00041440` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00042338` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00046226` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00053345` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00055008` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00064964` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00067574` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00070333` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00089428` | `geo_entropy_24h` | `user_id` | 2 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00000032` | `geo_entropy_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |
| 29 | `Txn` | `TxnByUser` | `k00000045` | `geo_entropy_24h` | `user_id` | 1 | 80 | 80 | 8 | 72 | 56 | 136 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 30. `time_since_last_n` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 30 | `Txn` | `TxnByUser` | `k00013835` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00017893` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00022821` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00029943` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00030706` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00030832` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00031894` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00039938` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00041244` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00041440` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00042338` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00046226` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00053345` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00055008` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00064964` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00067574` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00070333` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00089428` | `time_since_last_5` | `user_id` | 2 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00000032` | `time_since_last_5` | `user_id` | 1 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |
| 30 | `Txn` | `TxnByUser` | `k00000045` | `time_since_last_5` | `user_id` | 1 | 80 | 80 | 40 | 40 | 40 | 120 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 31. `last_seen` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 31 | `Txn` | `TxnByDevice` | `s377` | `device_last_seen` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s717` | `device_last_seen` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s743` | `device_last_seen` | `device_id` | 7 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s107` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s171` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s469` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s519` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s586` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s632` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s66` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s846` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s903` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s955` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s959` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s960` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s994` | `device_last_seen` | `device_id` | 6 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s116` | `device_last_seen` | `device_id` | 5 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s118` | `device_last_seen` | `device_id` | 5 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s136` | `device_last_seen` | `device_id` | 5 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |
| 31 | `Txn` | `TxnByDevice` | `s177` | `device_last_seen` | `device_id` | 5 | 80 | 80 | 32 | 48 | 0 | 80 | 0.0% |

Showing top 20 of `2843` entity-feature rows for this op/shape.

### 32. `negative_streak` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 32 | `Txn` | `TxnByCard` | `s409` | `decline_streak_card` | `card_fp` | 10 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s179` | `decline_streak_card` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s42` | `decline_streak_card` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s450` | `decline_streak_card` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s896` | `decline_streak_card` | `card_fp` | 7 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s138` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s274` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s337` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s354` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s433` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s474` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s555` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s560` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s566` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s654` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s770` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s957` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s996` | `decline_streak_card` | `card_fp` | 6 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s129` | `decline_streak_card` | `card_fp` | 5 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |
| 32 | `Txn` | `TxnByCard` | `s150` | `decline_streak_card` | `card_fp` | 5 | 80 | 80 | 8 | 72 | 0 | 80 | 0.0% |

Showing top 20 of `2836` entity-feature rows for this op/shape.

### 33. `count` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 33 | `Txn` | `TxnByUser` | `k00013835` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00017893` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00022821` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00029943` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00030706` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00030832` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00031894` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00039938` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00041244` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00041440` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00042338` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00046226` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00053345` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00055008` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00064964` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00067574` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00070333` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00089428` | `txn_count_lifetime` | `user_id` | 2 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00000032` | `txn_count_lifetime` | `user_id` | 1 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |
| 33 | `Txn` | `TxnByUser` | `k00000045` | `txn_count_lifetime` | `user_id` | 1 | 80 | 80 | 8 | 72 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 34. `decayed_count` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 34 | `Txn` | `TxnByUser` | `k00013835` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00017893` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00022821` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00029943` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00030706` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00030832` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00031894` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00039938` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00041244` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00041440` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00042338` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00046226` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00053345` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00055008` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00064964` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00067574` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00070333` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00089428` | `txn_decayed_count_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00000032` | `txn_decayed_count_24h` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 34 | `Txn` | `TxnByUser` | `k00000045` | `txn_decayed_count_24h` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 35. `decayed_sum` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 35 | `Txn` | `TxnByUser` | `k00013835` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00017893` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00022821` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00029943` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00030706` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00030832` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00031894` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00039938` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00041244` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00041440` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00042338` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00046226` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00053345` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00055008` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00064964` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00067574` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00070333` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00089428` | `amount_decayed_sum_24h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00000032` | `amount_decayed_sum_24h` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 35 | `Txn` | `TxnByUser` | `k00000045` | `amount_decayed_sum_24h` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 36. `delta_from_prev` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 36 | `Txn` | `TxnByUser` | `k00013835` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00017893` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00022821` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00029943` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00030706` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00030832` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00031894` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00039938` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00041244` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00041440` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00042338` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00046226` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00053345` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00055008` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00064964` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00067574` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00070333` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00089428` | `amount_delta` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00000032` | `amount_delta` | `user_id` | 1 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 36 | `Txn` | `TxnByUser` | `k00000045` | `amount_delta` | `user_id` | 1 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 37. `ew_zscore` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 37 | `Txn` | `TxnByUser` | `k00013835` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00017893` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00022821` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00029943` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00030706` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00030832` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00031894` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00039938` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00041244` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00041440` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00042338` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00046226` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00053345` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00055008` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00064964` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00067574` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00070333` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00089428` | `amount_ew_zscore` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00000032` | `amount_ew_zscore` | `user_id` | 1 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 37 | `Txn` | `TxnByUser` | `k00000045` | `amount_ew_zscore` | `user_id` | 1 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 38. `ewma` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 38 | `Txn` | `TxnByUser` | `k00013835` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00017893` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00022821` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00029943` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00030706` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00030832` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00031894` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00039938` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00041244` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00041440` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00042338` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00046226` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00053345` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00055008` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00064964` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00067574` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00070333` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00089428` | `amount_ewma_1h` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00000032` | `amount_ewma_1h` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 38 | `Txn` | `TxnByUser` | `k00000045` | `amount_ewma_1h` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 39. `ewvar` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 39 | `Txn` | `TxnByUser` | `k00013835` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00017893` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00022821` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00029943` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00030706` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00030832` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00031894` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00039938` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00041244` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00041440` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00042338` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00046226` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00053345` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00055008` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00064964` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00067574` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00070333` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00089428` | `amount_ewvar_1h` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00000032` | `amount_ewvar_1h` | `user_id` | 1 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 39 | `Txn` | `TxnByUser` | `k00000045` | `amount_ewvar_1h` | `user_id` | 1 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 40. `first` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 40 | `Txn` | `TxnByUser` | `k00013835` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00017893` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00022821` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00029943` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00030706` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00030832` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00031894` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00039938` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00041244` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00041440` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00042338` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00046226` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00053345` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00055008` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00064964` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00067574` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00070333` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00089428` | `first_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00000032` | `first_amount` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 40 | `Txn` | `TxnByUser` | `k00000045` | `first_amount` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 41. `first_seen_in_window` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 41 | `Txn` | `TxnByUser` | `k00013835` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00017893` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00022821` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00029943` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00030706` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00030832` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00031894` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00039938` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00041244` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00041440` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00042338` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00046226` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00053345` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00055008` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00064964` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00067574` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00070333` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00089428` | `first_in_24h` | `user_id` | 2 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00000032` | `first_in_24h` | `user_id` | 1 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |
| 41 | `Txn` | `TxnByUser` | `k00000045` | `first_in_24h` | `user_id` | 1 | 80 | 80 | 24 | 56 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 42. `has_seen` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 42 | `Txn` | `TxnByUser` | `k00013835` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00017893` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00022821` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00029943` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00030706` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00030832` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00031894` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00039938` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00041244` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00041440` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00042338` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00046226` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00053345` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00055008` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00064964` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00067574` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00070333` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00089428` | `has_seen` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00000032` | `has_seen` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 42 | `Txn` | `TxnByUser` | `k00000045` | `has_seen` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 43. `inter_arrival_stats` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 43 | `Txn` | `TxnByUser` | `k00013835` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00017893` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00022821` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00029943` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00030706` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00030832` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00031894` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00039938` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00041244` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00041440` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00042338` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00046226` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00053345` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00055008` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00064964` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00067574` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00070333` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00089428` | `inter_arrival_1h` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00000032` | `inter_arrival_1h` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 43 | `Txn` | `TxnByUser` | `k00000045` | `inter_arrival_1h` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 44. `last` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 44 | `Txn` | `TxnByUser` | `k00013835` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00017893` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00022821` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00029943` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00030706` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00030832` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00031894` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00039938` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00041244` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00041440` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00042338` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00046226` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00053345` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00055008` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00064964` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00067574` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00070333` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00089428` | `last_amount` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00000032` | `last_amount` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 44 | `Txn` | `TxnByUser` | `k00000045` | `last_amount` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 45. `max` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 45 | `Txn` | `TxnByUser` | `k00013835` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00017893` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00022821` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00029943` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00030706` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00030832` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00031894` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00039938` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00041244` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00041440` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00042338` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00046226` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00053345` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00055008` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00064964` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00067574` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00070333` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00089428` | `max_amount_lifetime` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00000032` | `max_amount_lifetime` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 45 | `Txn` | `TxnByUser` | `k00000045` | `max_amount_lifetime` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 46. `max_streak` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 46 | `Txn` | `TxnByUser` | `k00013835` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00017893` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00022821` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00029943` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00030706` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00030832` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00031894` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00039938` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00041244` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00041440` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00042338` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00046226` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00053345` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00055008` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00064964` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00067574` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00070333` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00089428` | `max_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00000032` | `max_streak` | `user_id` | 1 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 46 | `Txn` | `TxnByUser` | `k00000045` | `max_streak` | `user_id` | 1 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 47. `outlier_count` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 47 | `Txn` | `TxnByUser` | `k00013835` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00017893` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00022821` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00029943` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00030706` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00030832` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00031894` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00039938` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00041244` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00041440` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00042338` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00046226` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00053345` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00055008` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00064964` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00067574` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00070333` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00089428` | `amount_outliers_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00000032` | `amount_outliers_5m` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 47 | `Txn` | `TxnByUser` | `k00000045` | `amount_outliers_5m` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 48. `rate_of_change` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 48 | `Txn` | `TxnByUser` | `k00013835` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00017893` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00022821` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00029943` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00030706` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00030832` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00031894` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00039938` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00041244` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00041440` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00042338` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00046226` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00053345` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00055008` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00064964` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00067574` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00070333` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00089428` | `amount_rate_5m` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00000032` | `amount_rate_5m` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 48 | `Txn` | `TxnByUser` | `k00000045` | `amount_rate_5m` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 49. `streak` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 49 | `Txn` | `TxnByUser` | `k00013835` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00017893` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00022821` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00029943` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00030706` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00030832` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00031894` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00039938` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00041244` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00041440` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00042338` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00046226` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00053345` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00055008` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00064964` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00067574` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00070333` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00089428` | `txn_streak` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00000032` | `txn_streak` | `user_id` | 1 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 49 | `Txn` | `TxnByUser` | `k00000045` | `txn_streak` | `user_id` | 1 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 50. `sum` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 50 | `Txn` | `TxnByUser` | `k00013835` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00017893` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00022821` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00029943` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00030706` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00030832` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00031894` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00039938` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00041244` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00041440` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00042338` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00046226` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00053345` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00055008` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00064964` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00067574` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00070333` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00089428` | `sum_amount_lifetime` | `user_id` | 2 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00000032` | `sum_amount_lifetime` | `user_id` | 1 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |
| 50 | `Txn` | `TxnByUser` | `k00000045` | `sum_amount_lifetime` | `user_id` | 1 | 80 | 80 | 16 | 64 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 51. `time_since` / `lifetime` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 51 | `Txn` | `TxnByUser` | `k00013835` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00017893` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00022821` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00029943` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00030706` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00030832` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00031894` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00039938` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00041244` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00041440` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00042338` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00046226` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00053345` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00055008` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00064964` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00067574` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00070333` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00089428` | `time_since_last` | `user_id` | 2 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00000032` | `time_since_last` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |
| 51 | `Txn` | `TxnByUser` | `k00000045` | `time_since_last` | `user_id` | 1 | 80 | 80 | 32 | 48 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 52. `trend` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 52 | `Txn` | `TxnByUser` | `k00013835` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00017893` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00022821` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00029943` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00030706` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00030832` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00031894` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00039938` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00041244` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00041440` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00042338` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00046226` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00053345` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00055008` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00064964` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00067574` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00070333` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00089428` | `amount_trend_5m` | `user_id` | 2 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00000032` | `amount_trend_5m` | `user_id` | 1 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |
| 52 | `Txn` | `TxnByUser` | `k00000045` | `amount_trend_5m` | `user_id` | 1 | 80 | 80 | 48 | 32 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 53. `trend_residual` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 53 | `Txn` | `TxnByUser` | `k00013835` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00017893` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00022821` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00029943` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00030706` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00030832` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00031894` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00039938` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00041244` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00041440` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00042338` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00046226` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00053345` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00055008` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00064964` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00067574` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00070333` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00089428` | `amount_trend_resid_5m` | `user_id` | 2 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00000032` | `amount_trend_resid_5m` | `user_id` | 1 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |
| 53 | `Txn` | `TxnByUser` | `k00000045` | `amount_trend_resid_5m` | `user_id` | 1 | 80 | 80 | 72 | 8 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 54. `twa` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 54 | `Txn` | `TxnByUser` | `k00013835` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00017893` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00022821` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00029943` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00030706` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00030832` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00031894` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00039938` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00041244` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00041440` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00042338` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00046226` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00053345` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00055008` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00064964` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00067574` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00070333` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00089428` | `amount_twa_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00000032` | `amount_twa_5m` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 54 | `Txn` | `TxnByUser` | `k00000045` | `amount_twa_5m` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 55. `value_change_count` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 55 | `Txn` | `TxnByUser` | `k00013835` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00017893` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00022821` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00029943` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00030706` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00030832` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00031894` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00039938` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00041244` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00041440` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00042338` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00046226` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00053345` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00055008` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00064964` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00067574` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00070333` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00089428` | `device_change_count_5m` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00000032` | `device_change_count_5m` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 55 | `Txn` | `TxnByUser` | `k00000045` | `device_change_count_5m` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

### 56. `z_score` / `windowed` entity-feature rows

| Parent rank | Source event | Derivation | Entity key | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 56 | `Txn` | `TxnByUser` | `k00013835` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00017893` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00022821` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00029943` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00030706` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00030832` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00031894` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00039938` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00041244` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00041440` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00042338` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00046226` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00053345` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00055008` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00064964` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00067574` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00070333` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00089428` | `amount_z_score` | `user_id` | 2 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00000032` | `amount_z_score` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |
| 56 | `Txn` | `TxnByUser` | `k00000045` | `amount_z_score` | `user_id` | 1 | 80 | 80 | 40 | 40 | 0 | 80 | 0.1% |

Showing top 20 of `1982` entity-feature rows for this op/shape.

## Top 5 Offenders

### 1. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s470`
- Entity events: `9`
- Key path: `merchant_id`
- Events applied: `9`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2416 total=2496
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Percentile exact samples across buckets`: 2048 bytes (Vec, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<PercentileStateWrap> across buckets`: 112 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<PercentileStateWrap>`: 112 bytes (Box, heap allocation for boxed Percentile wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 2. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s176`
- Entity events: `7`
- Key path: `merchant_id`
- Events applied: `7`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2416 total=2496
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Percentile exact samples across buckets`: 2048 bytes (Vec, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<PercentileStateWrap> across buckets`: 112 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<PercentileStateWrap>`: 112 bytes (Box, heap allocation for boxed Percentile wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 3. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s278`
- Entity events: `7`
- Key path: `merchant_id`
- Events applied: `7`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2416 total=2496
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Percentile exact samples across buckets`: 2048 bytes (Vec, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<PercentileStateWrap> across buckets`: 112 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<PercentileStateWrap>`: 112 bytes (Box, heap allocation for boxed Percentile wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 4. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s387`
- Entity events: `7`
- Key path: `merchant_id`
- Events applied: `7`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2416 total=2496
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Percentile exact samples across buckets`: 2048 bytes (Vec, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<PercentileStateWrap> across buckets`: 112 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<PercentileStateWrap>`: 112 bytes (Box, heap allocation for boxed Percentile wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 5. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s507`
- Entity events: `7`
- Key path: `merchant_id`
- Events applied: `7`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2416 total=2496
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Percentile exact samples across buckets`: 2048 bytes (Vec, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<PercentileStateWrap> across buckets`: 112 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / Percentile exact samples`: 2048 bytes (Vec, capacity * size_of::<f64>() for exact percentile samples)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<PercentileStateWrap>`: 112 bytes (Box, heap allocation for boxed Percentile wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile bytes-per-active-entity-row p99: `26655` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 19655 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- `enum_slot_bytes` is the fixed-size `AggOp` enum slot charged to a row; parent rows sum this across child paths.
- `payload_bytes` is the active variant payload inside the enum slot. For boxed variants this is the inline `Box<T>` pointer, while the boxed pointee remains in `heap_bytes`.
- `slack_bytes` is unused capacity in the fixed-size `AggOp` enum slot: `enum_slot_bytes - payload_bytes`.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
- Primary grain is `derivation table -> entity row -> feature column`; op/shape rows remain as secondary diagnostics for implementation-level hotspots.
