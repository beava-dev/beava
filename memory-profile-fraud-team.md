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
