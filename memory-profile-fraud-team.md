# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `fraud`
- Events requested from generator: `50000`
- Events replayed from generator: `50000`
- Events by source:
  - `Txn`: `50000`
- Derivations discovered: `9`
- Aggregate features discovered: `111`
- Active entity rows profiled: `43421`
- Bytes per active entity row p99: `62816` bytes

## Per-Entity Table Footprint

| Rank | Table | Source | group_by key | Active entities | Features/entity | Events applied | Stack p50 | Stack p99 | Stack max | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max | Top contributor |
|------|-------|--------|--------------|-----------------|-----------------|----------------|-----------|-----------|-----------|----------|----------|----------|-----------|-----------|-----------|-----------------|
| 1 | `TxnByIp` | `Txn` | `ip_address` | 1000 | 8 | 50000 | 640 | 640 | 640 | 61984 | 63712 | 64576 | 62624 | 64352 | 65216 | `cards_per_ip_1h` |
| 2 | `TxnByDevice` | `Txn` | `device_id` | 1000 | 6 | 50000 | 480 | 480 | 480 | 37728 | 37728 | 37728 | 38208 | 38208 | 38208 | `cards_per_device_24h` |
| 3 | `TxnByUser` | `Txn` | `user_id` | 39421 | 62 | 50000 | 4960 | 4960 | 4960 | 20494 | 21254 | 22190 | 25454 | 26214 | 27150 | `amount_p95_24h` |
| 4 | `TxnByMerchant` | `Txn` | `merchant_id` | 1000 | 4 | 50000 | 320 | 320 | 320 | 21408 | 21408 | 21408 | 21728 | 21728 | 21728 | `users_per_merchant_24h` |
| 5 | `TxnByCard` | `Txn` | `card_fp` | 1000 | 8 | 50000 | 640 | 640 | 640 | 20688 | 20688 | 20688 | 21328 | 21328 | 21328 | `merchants_per_card_24h` |
| 6 | `CardAddByDevice` | `CardAdd` | `device_id` | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 7 | `LoginByUser` | `Login` | `user_id` | 0 | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 8 | `RefundByUser` | `Refund` | `user_id` | 0 | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 9 | `SignupByIp` | `Signup` | `ip_address` | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |

## AggOp Payload Bytes Plot

Unique AggOp kinds grouped by max observed inline `payload_bytes` in 8-byte bands (`0-8`, `9-16`, ...). Bars use `#`; the detail table marks bands containing payloads at or above 48 bytes as boxing candidates.

```text
Payload band | Op count | Plot
-------------|----------|----------------------------------
     0-8   B |       18 | ################
     9-16  B |        3 | ###
    17-24  B |        3 | ###
    25-32  B |       13 | ############
    33-40  B |        9 | ########
    41-48  B |        4 | ####
    49-56  B |        0 |
    57-64  B |        0 |
    65-72  B |        2 | ##
```

| Payload band | Boxing candidate | Op count | AggOps |
|--------------|------------------|----------|--------|
| 0-8 B | no | 18 | `bloom_member(8B)`, `count(8B)`, `distance_from_home(8B)`, `entropy(8B)`, `event_type_mix(8B)`, `geo_distance(8B)`, `geo_spread(8B)`, `geo_velocity(8B)`, `hour_of_day_histogram(8B)`, `mean(8B)`, `min(8B)`, `n_unique(8B)`, `negative_streak(8B)`, `quantile(8B)`, `seasonal_deviation(8B)`, `std(8B)`, `top_k(8B)`, `var(8B)` |
| 9-16 B | no | 3 | `max_streak(16B)`, `streak(16B)`, `sum(16B)` |
| 17-24 B | no | 3 | `delta_from_prev(24B)`, `dow_hour_histogram(24B)`, `first_seen_in_window(24B)` |
| 25-32 B | no | 13 | `age(32B)`, `decayed_count(32B)`, `decayed_sum(32B)`, `ewma(32B)`, `first(32B)`, `first_n(32B)`, `first_seen(32B)`, `has_seen(32B)`, `last(32B)`, `last_seen(32B)`, `max(32B)`, `rate_of_change(32B)`, `time_since(32B)` |
| 33-40 B | no | 9 | `inter_arrival_stats(40B)`, `lag(40B)`, `last_n(40B)`, `outlier_count(40B)`, `reservoir_sample(40B)`, `time_since_last_n(40B)`, `twa(40B)`, `value_change_count(40B)`, `z_score(40B)` |
| 41-48 B | yes | 4 | `ew_zscore(48B)`, `ewvar(48B)`, `most_recent_n(48B)`, `trend(48B)` |
| 49-56 B | no | 0 | - |
| 57-64 B | no | 0 | - |
| 65-72 B | yes | 2 | `burst_count(72B)`, `trend_residual(72B)` |

## Per-Table Entity Details

### `TxnByIp` (`Txn` by `ip_address`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `cards_per_ip_1h` | `n_unique` | `windowed` | 80 | 37296 | 37296 | 37296 | 37376 | 37376 | 37376 |
| `users_per_ip_24h` | `n_unique` | `windowed` | 80 | 18736 | 18736 | 18736 | 18816 | 18816 | 18816 |
| `ip_top_users` | `top_k` | `windowed` | 80 | 5216 | 6752 | 7616 | 5296 | 6832 | 7696 |
| `amount_sum_per_ip_1h` | `sum` | `windowed` | 80 | 336 | 336 | 336 | 416 | 416 | 416 |
| `txn_per_ip_1h` | `count` | `windowed` | 80 | 336 | 336 | 336 | 416 | 416 | 416 |
| `txn_per_ip_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `ip_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `ip_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s974` | 75 | 640 | 64576 | 65216 | `cards_per_ip_1h`=37376 bytes, `users_per_ip_24h`=18816 bytes, `ip_top_users`=7696 bytes |
| `s533` | 71 | 640 | 64192 | 64832 | `cards_per_ip_1h`=37376 bytes, `users_per_ip_24h`=18816 bytes, `ip_top_users`=7312 bytes |
| `s58` | 70 | 640 | 64096 | 64736 | `cards_per_ip_1h`=37376 bytes, `users_per_ip_24h`=18816 bytes, `ip_top_users`=7216 bytes |
| `s687` | 70 | 640 | 64096 | 64736 | `cards_per_ip_1h`=37376 bytes, `users_per_ip_24h`=18816 bytes, `ip_top_users`=7216 bytes |
| `s24` | 68 | 640 | 63904 | 64544 | `cards_per_ip_1h`=37376 bytes, `users_per_ip_24h`=18816 bytes, `ip_top_users`=7024 bytes |

#### Feature Breakdown For Largest Entity `s974`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `cards_per_ip_1h` | `n_unique` | `windowed` | 75 | 80 | 80 | 8 | 72 | 37296 | 37376 |
| `users_per_ip_24h` | `n_unique` | `windowed` | 75 | 80 | 80 | 8 | 72 | 18736 | 18816 |
| `ip_top_users` | `top_k` | `windowed` | 75 | 80 | 80 | 8 | 72 | 7616 | 7696 |
| `amount_sum_per_ip_1h` | `sum` | `windowed` | 75 | 80 | 80 | 8 | 72 | 336 | 416 |
| `txn_per_ip_1h` | `count` | `windowed` | 75 | 80 | 80 | 8 | 72 | 336 | 416 |
| `txn_per_ip_24h` | `count` | `windowed` | 75 | 80 | 80 | 8 | 72 | 256 | 336 |
| `ip_age` | `age` | `lifetime` | 75 | 80 | 80 | 32 | 48 | 0 | 80 |
| `ip_first_seen` | `first_seen` | `lifetime` | 75 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByDevice` (`Txn` by `device_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `cards_per_device_24h` | `n_unique` | `windowed` | 80 | 18736 | 18736 | 18736 | 18816 | 18816 | 18816 |
| `users_per_device_24h` | `n_unique` | `windowed` | 80 | 18736 | 18736 | 18736 | 18816 | 18816 | 18816 |
| `device_txn_count_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `device_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_last_seen` | `last_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s960` | 73 | 480 | 37728 | 38208 | `cards_per_device_24h`=18816 bytes, `users_per_device_24h`=18816 bytes, `device_txn_count_24h`=336 bytes |
| `s361` | 72 | 480 | 37728 | 38208 | `cards_per_device_24h`=18816 bytes, `users_per_device_24h`=18816 bytes, `device_txn_count_24h`=336 bytes |
| `s641` | 71 | 480 | 37728 | 38208 | `cards_per_device_24h`=18816 bytes, `users_per_device_24h`=18816 bytes, `device_txn_count_24h`=336 bytes |
| `s966` | 71 | 480 | 37728 | 38208 | `cards_per_device_24h`=18816 bytes, `users_per_device_24h`=18816 bytes, `device_txn_count_24h`=336 bytes |
| `s141` | 70 | 480 | 37728 | 38208 | `cards_per_device_24h`=18816 bytes, `users_per_device_24h`=18816 bytes, `device_txn_count_24h`=336 bytes |

#### Feature Breakdown For Largest Entity `s960`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `cards_per_device_24h` | `n_unique` | `windowed` | 73 | 80 | 80 | 8 | 72 | 18736 | 18816 |
| `users_per_device_24h` | `n_unique` | `windowed` | 73 | 80 | 80 | 8 | 72 | 18736 | 18816 |
| `device_txn_count_24h` | `count` | `windowed` | 73 | 80 | 80 | 8 | 72 | 256 | 336 |
| `device_age` | `age` | `lifetime` | 73 | 80 | 80 | 32 | 48 | 0 | 80 |
| `device_first_seen` | `first_seen` | `lifetime` | 73 | 80 | 80 | 32 | 48 | 0 | 80 |
| `device_last_seen` | `last_seen` | `lifetime` | 73 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByUser` (`Txn` by `user_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `amount_p95_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `p50_amount_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `p99_amount_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `dist_from_home` | `distance_from_home` | `lifetime` | 80 | 1720 | 1720 | 1720 | 1800 | 1800 | 1800 |
| `reservoir_50` | `reservoir_sample` | `lifetime` | 80 | 1600 | 1600 | 1600 | 1680 | 1680 | 1680 |
| `dow_hour_hist_30d` | `dow_hour_histogram` | `lifetime` | 80 | 1344 | 1344 | 1344 | 1424 | 1424 | 1424 |
| `device_seen` | `bloom_member` | `lifetime` | 80 | 1280 | 1280 | 1280 | 1360 | 1360 | 1360 |
| `burst_count_5m` | `burst_count` | `windowed` | 80 | 1024 | 1024 | 1024 | 1104 | 1104 | 1104 |
| `top_merchants_24h` | `top_k` | `windowed` | 80 | 512 | 704 | 896 | 592 | 784 | 976 |
| `seasonal_dev` | `seasonal_deviation` | `lifetime` | 80 | 600 | 600 | 600 | 680 | 680 | 680 |
| `mcc_entropy_24h` | `entropy` | `windowed` | 80 | 396 | 564 | 732 | 476 | 644 | 812 |
| `countries_distinct_7d` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `ips_distinct_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `merchants_distinct_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `txn_count_5m` | `count` | `windowed` | 80 | 256 | 416 | 704 | 336 | 496 | 784 |
| `event_mix_24h` | `event_type_mix` | `lifetime` | 80 | 208 | 368 | 528 | 288 | 448 | 608 |
| `txn_count_1h` | `count` | `windowed` | 80 | 256 | 336 | 336 | 336 | 416 | 416 |
| `avg_amount_24h` | `mean` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `min_amount_24h` | `min` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `std_amount_24h` | `std` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `sum_amount_24h` | `sum` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_count_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `var_amount_24h` | `var` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `hour_hist_30d` | `hour_of_day_histogram` | `lifetime` | 80 | 192 | 192 | 192 | 272 | 272 | 272 |
| `unique_cells_24h` | `n_unique` | `lifetime` | 80 | 168 | 168 | 168 | 248 | 248 | 248 |
| `last_5_amounts` | `last_n` | `lifetime` | 80 | 160 | 160 | 160 | 240 | 240 | 240 |
| `recent_5_amts` | `most_recent_n` | `lifetime` | 80 | 160 | 160 | 160 | 240 | 240 | 240 |
| `first_5_merchants` | `first_n` | `lifetime` | 80 | 128 | 128 | 256 | 208 | 208 | 336 |
| `geo_kmh` | `geo_velocity` | `lifetime` | 80 | 94 | 94 | 94 | 174 | 174 | 174 |
| `geo_spread_24h` | `geo_spread` | `lifetime` | 80 | 94 | 94 | 94 | 174 | 174 | 174 |
| `geo_dist_last` | `geo_distance` | `lifetime` | 80 | 86 | 86 | 86 | 166 | 166 | 166 |
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
| `k00004886` | 5 | 4960 | 22190 | 27150 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00026752` | 5 | 4960 | 22190 | 27150 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00074966` | 5 | 4960 | 22190 | 27150 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00061001` | 5 | 4960 | 22189 | 27149 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00073588` | 5 | 4960 | 22189 | 27149 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |

#### Feature Breakdown For Largest Entity `k00004886`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `amount_p95_24h` | `quantile` | `windowed` | 5 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `p50_amount_24h` | `quantile` | `windowed` | 5 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `p99_amount_24h` | `quantile` | `windowed` | 5 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `dist_from_home` | `distance_from_home` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 1720 | 1800 |
| `reservoir_50` | `reservoir_sample` | `lifetime` | 5 | 80 | 80 | 40 | 40 | 1600 | 1680 |
| `dow_hour_hist_30d` | `dow_hour_histogram` | `lifetime` | 5 | 80 | 80 | 24 | 56 | 1344 | 1424 |
| `device_seen` | `bloom_member` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 1280 | 1360 |
| `burst_count_5m` | `burst_count` | `windowed` | 5 | 80 | 80 | 72 | 8 | 1024 | 1104 |
| `top_merchants_24h` | `top_k` | `windowed` | 5 | 80 | 80 | 8 | 72 | 896 | 976 |
| `mcc_entropy_24h` | `entropy` | `windowed` | 5 | 80 | 80 | 8 | 72 | 732 | 812 |
| `txn_count_5m` | `count` | `windowed` | 5 | 80 | 80 | 8 | 72 | 704 | 784 |
| `seasonal_dev` | `seasonal_deviation` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 600 | 680 |
| `event_mix_24h` | `event_type_mix` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 528 | 608 |
| `countries_distinct_7d` | `n_unique` | `windowed` | 5 | 80 | 80 | 8 | 72 | 424 | 504 |
| `ips_distinct_24h` | `n_unique` | `windowed` | 5 | 80 | 80 | 8 | 72 | 424 | 504 |
| `merchants_distinct_24h` | `n_unique` | `windowed` | 5 | 80 | 80 | 8 | 72 | 424 | 504 |
| `txn_count_1h` | `count` | `windowed` | 5 | 80 | 80 | 8 | 72 | 336 | 416 |
| `avg_amount_24h` | `mean` | `windowed` | 5 | 80 | 80 | 8 | 72 | 256 | 336 |
| `first_5_merchants` | `first_n` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 256 | 336 |
| `min_amount_24h` | `min` | `windowed` | 5 | 80 | 80 | 8 | 72 | 256 | 336 |
| `std_amount_24h` | `std` | `windowed` | 5 | 80 | 80 | 8 | 72 | 256 | 336 |
| `sum_amount_24h` | `sum` | `windowed` | 5 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_count_24h` | `count` | `windowed` | 5 | 80 | 80 | 8 | 72 | 256 | 336 |
| `var_amount_24h` | `var` | `windowed` | 5 | 80 | 80 | 8 | 72 | 256 | 336 |
| `hour_hist_30d` | `hour_of_day_histogram` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 192 | 272 |
| `unique_cells_24h` | `n_unique` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 168 | 248 |
| `last_5_amounts` | `last_n` | `lifetime` | 5 | 80 | 80 | 40 | 40 | 160 | 240 |
| `recent_5_amts` | `most_recent_n` | `lifetime` | 5 | 80 | 80 | 48 | 32 | 160 | 240 |
| `geo_kmh` | `geo_velocity` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 94 | 174 |
| `geo_spread_24h` | `geo_spread` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 94 | 174 |
| `geo_dist_last` | `geo_distance` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 86 | 166 |
| `amount_lag1` | `lag` | `lifetime` | 5 | 80 | 80 | 40 | 40 | 64 | 144 |
| `geo_entropy_24h` | `entropy` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 56 | 136 |
| `time_since_last_5` | `time_since_last_n` | `lifetime` | 5 | 80 | 80 | 40 | 40 | 40 | 120 |
| `age` | `age` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_decayed_sum_24h` | `decayed_sum` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_delta` | `delta_from_prev` | `lifetime` | 5 | 80 | 80 | 24 | 56 | 0 | 80 |
| `amount_ew_zscore` | `ew_zscore` | `lifetime` | 5 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_ewma_1h` | `ewma` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_ewvar_1h` | `ewvar` | `lifetime` | 5 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_outliers_5m` | `outlier_count` | `windowed` | 5 | 80 | 80 | 40 | 40 | 0 | 80 |
| `amount_rate_5m` | `rate_of_change` | `windowed` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_trend_5m` | `trend` | `windowed` | 5 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_trend_resid_5m` | `trend_residual` | `windowed` | 5 | 80 | 80 | 72 | 8 | 0 | 80 |
| `amount_twa_5m` | `twa` | `windowed` | 5 | 80 | 80 | 40 | 40 | 0 | 80 |
| `amount_z_score` | `z_score` | `windowed` | 5 | 80 | 80 | 40 | 40 | 0 | 80 |
| `decline_streak` | `negative_streak` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 0 | 80 |
| `device_change_count_5m` | `value_change_count` | `windowed` | 5 | 80 | 80 | 40 | 40 | 0 | 80 |
| `first_amount` | `first` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `first_in_24h` | `first_seen_in_window` | `windowed` | 5 | 80 | 80 | 24 | 56 | 0 | 80 |
| `first_seen` | `first_seen` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `has_seen` | `has_seen` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `inter_arrival_1h` | `inter_arrival_stats` | `windowed` | 5 | 80 | 80 | 40 | 40 | 0 | 80 |
| `last_amount` | `last` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `last_seen` | `last_seen` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `max_amount_lifetime` | `max` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `max_streak` | `max_streak` | `lifetime` | 5 | 80 | 80 | 16 | 64 | 0 | 80 |
| `sum_amount_lifetime` | `sum` | `lifetime` | 5 | 80 | 80 | 16 | 64 | 0 | 80 |
| `time_since_last` | `time_since` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `txn_count_lifetime` | `count` | `lifetime` | 5 | 80 | 80 | 8 | 72 | 0 | 80 |
| `txn_decayed_count_24h` | `decayed_count` | `lifetime` | 5 | 80 | 80 | 32 | 48 | 0 | 80 |
| `txn_streak` | `streak` | `lifetime` | 5 | 80 | 80 | 16 | 64 | 0 | 80 |

### `TxnByMerchant` (`Txn` by `merchant_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `users_per_merchant_24h` | `n_unique` | `windowed` | 80 | 18736 | 18736 | 18736 | 18816 | 18816 | 18816 |
| `merchant_amount_p99_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `txn_per_merchant_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `merchant_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s370` | 73 | 320 | 21408 | 21728 | `users_per_merchant_24h`=18816 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s969` | 73 | 320 | 21408 | 21728 | `users_per_merchant_24h`=18816 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s584` | 71 | 320 | 21408 | 21728 | `users_per_merchant_24h`=18816 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s304` | 69 | 320 | 21408 | 21728 | `users_per_merchant_24h`=18816 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s682` | 69 | 320 | 21408 | 21728 | `users_per_merchant_24h`=18816 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |

#### Feature Breakdown For Largest Entity `s370`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `users_per_merchant_24h` | `n_unique` | `windowed` | 73 | 80 | 80 | 8 | 72 | 18736 | 18816 |
| `merchant_amount_p99_24h` | `quantile` | `windowed` | 73 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `txn_per_merchant_24h` | `count` | `windowed` | 73 | 80 | 80 | 8 | 72 | 256 | 336 |
| `merchant_first_seen` | `first_seen` | `lifetime` | 73 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByCard` (`Txn` by `card_fp`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `merchants_per_card_24h` | `n_unique` | `windowed` | 80 | 18736 | 18736 | 18736 | 18816 | 18816 | 18816 |
| `small_amt_burst_5m` | `burst_count` | `windowed` | 80 | 1024 | 1024 | 1024 | 1104 | 1104 | 1104 |
| `decline_count_1h` | `count` | `windowed` | 80 | 336 | 336 | 336 | 416 | 416 | 416 |
| `txn_per_card_1h` | `count` | `windowed` | 80 | 336 | 336 | 336 | 416 | 416 | 416 |
| `txn_per_card_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `card_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `card_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `decline_streak_card` | `negative_streak` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s372` | 72 | 640 | 20688 | 21328 | `merchants_per_card_24h`=18816 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=416 bytes |
| `s42` | 70 | 640 | 20688 | 21328 | `merchants_per_card_24h`=18816 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=416 bytes |
| `s907` | 70 | 640 | 20688 | 21328 | `merchants_per_card_24h`=18816 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=416 bytes |
| `s214` | 69 | 640 | 20688 | 21328 | `merchants_per_card_24h`=18816 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=416 bytes |
| `s272` | 69 | 640 | 20688 | 21328 | `merchants_per_card_24h`=18816 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=416 bytes |

#### Feature Breakdown For Largest Entity `s372`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `merchants_per_card_24h` | `n_unique` | `windowed` | 72 | 80 | 80 | 8 | 72 | 18736 | 18816 |
| `small_amt_burst_5m` | `burst_count` | `windowed` | 72 | 80 | 80 | 72 | 8 | 1024 | 1104 |
| `decline_count_1h` | `count` | `windowed` | 72 | 80 | 80 | 8 | 72 | 336 | 416 |
| `txn_per_card_1h` | `count` | `windowed` | 72 | 80 | 80 | 8 | 72 | 336 | 416 |
| `txn_per_card_24h` | `count` | `windowed` | 72 | 80 | 80 | 8 | 72 | 256 | 336 |
| `card_age` | `age` | `lifetime` | 72 | 80 | 80 | 32 | 48 | 0 | 80 |
| `card_first_seen` | `first_seen` | `lifetime` | 72 | 80 | 80 | 32 | 48 | 0 | 80 |
| `decline_streak_card` | `negative_streak` | `lifetime` | 72 | 80 | 80 | 8 | 72 | 0 | 80 |

### `CardAddByDevice` (`CardAdd` by `device_id`)

No active entity rows. Configured features: `3`. The workload generator emitted no events for this table's source.

### `LoginByUser` (`Login` by `user_id`)

No active entity rows. Configured features: `8`. The workload generator emitted no events for this table's source.

### `RefundByUser` (`Refund` by `user_id`)

No active entity rows. Configured features: `8`. The workload generator emitted no events for this table's source.

### `SignupByIp` (`Signup` by `ip_address`)

No active entity rows. Configured features: `4`. The workload generator emitted no events for this table's source.

## Top 5 Offenders

One heaviest entity-feature example per unique op.

### 1. `Txn` / `TxnByIp` / `cards_per_ip_1h` / `n_unique`

- Path: `Txn` -> `TxnByIp` -> `cards_per_ip_1h` -> `n_unique` -> `windowed`
- Entity key: `s974`
- Entity events: `75`
- Key path: `ip_address`
- Events applied: `75`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=37296 total=37376
- Shape: `windowed` (1h)
- Breakdown rollup:
  - `CountDistinct hash-set slots across buckets`: 36880 bytes (HashSet, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Windowed bucket shell overhead`: 160 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Box<CountDistinctStateWrap> across buckets`: 80 bytes (Box, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 0 / CountDistinct hash-set slots`: 18440 bytes (HashSet, usage_limit=1024; requested_capacity=1024; hashbrown_usable_capacity=1792; inferred_backing_buckets=2048;)
  - `Windowed bucket 1 / CountDistinct hash-set slots`: 18440 bytes (HashSet, usage_limit=1024; requested_capacity=1024; hashbrown_usable_capacity=1792; inferred_backing_buckets=2048;)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 1 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<CountDistinctStateWrap>`: 40 bytes (Box, heap allocation for boxed CountDistinct wrapper)
  - `Windowed bucket 1 / Box<CountDistinctStateWrap>`: 40 bytes (Box, heap allocation for boxed CountDistinct wrapper)

### 2. `Txn` / `TxnByIp` / `ip_top_users` / `top_k`

- Path: `Txn` -> `TxnByIp` -> `ip_top_users` -> `top_k` -> `windowed`
- Entity key: `s974`
- Entity events: `75`
- Key path: `ip_address`
- Events applied: `75`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=7616 total=7696
- Shape: `windowed` (1d)
- Breakdown rollup:
  - `TopK exact BTreeMap entries across buckets`: 7200 bytes (BTreeMap, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<TopKStateWrap> across buckets`: 160 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / TopK exact BTreeMap entries`: 7200 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 3. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s370`
- Entity events: `73`
- Key path: `merchant_id`
- Events applied: `73`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2416 total=2496
- Shape: `windowed` (1d)
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

### 4. `Txn` / `TxnByUser` / `dist_from_home` / `distance_from_home`

- Path: `Txn` -> `TxnByUser` -> `dist_from_home` -> `distance_from_home` -> `lifetime`
- Entity key: `k00004886`
- Entity events: `5`
- Key path: `user_id`
- Events applied: `5`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=1720 total=1800
- Shape: `lifetime`
- Breakdown:
  - `DistanceFromHome coordinate buffer`: 1600 bytes (Vec, capacity * size_of::<(f64, f64)>())
  - `Box<DistanceFromHomeState>`: 120 bytes (Box, heap allocation for boxed payload)

### 5. `Txn` / `TxnByUser` / `reservoir_50` / `reservoir_sample`

- Path: `Txn` -> `TxnByUser` -> `reservoir_50` -> `reservoir_sample` -> `lifetime`
- Entity key: `k00004886`
- Entity events: `5`
- Key path: `user_id`
- Events applied: `5`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=40 slack_bytes=40) heap=1600 total=1680
- Shape: `lifetime`
- Breakdown:
  - `ReservoirSample reservoir`: 1600 bytes (Vec, capacity * size_of::<Value>())

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile bytes-per-active-entity-row p99: `62816` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 55816 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- `enum_slot_bytes` is the fixed-size `AggOp` enum slot charged to a row; parent rows sum this across child paths.
- `payload_bytes` is the active variant payload inside the enum slot. For boxed variants this is the inline `Box<T>` pointer, while the boxed pointee remains in `heap_bytes`.
- `slack_bytes` is unused capacity in the fixed-size `AggOp` enum slot: `enum_slot_bytes - payload_bytes`.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
- `AggOp` state is replayed through production `Registry` + `StateTables`; event counts come from a memprofile-only sidecar counter.
- Primary grain is `derivation table -> entity row -> feature column`; top offenders list one concrete entity-feature row per unique op.
