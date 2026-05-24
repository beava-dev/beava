# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `fraud`
- Events requested from generator: `10000`
- Events replayed from generator: `10000`
- Events by source:
  - `Txn`: `10000`
- Derivations discovered: `9`
- Aggregate features discovered: `111`
- Active entity rows profiled: `13533`
- Bytes per active entity row p99: `25794` bytes

## Per-Entity Table Footprint

| Rank | Table | Source | group_by key | Active entities | Features/entity | Events applied | Stack p50 | Stack p99 | Stack max | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max | Top contributor |
|------|-------|--------|--------------|-----------------|-----------------|----------------|-----------|-----------|-----------|----------|----------|----------|-----------|-----------|-----------|-----------------|
| 1 | `TxnByIp` | `Txn` | `ip_address` | 1000 | 8 | 10000 | 640 | 640 | 640 | 2992 | 60848 | 61136 | 3632 | 61488 | 61776 | `cards_per_ip_1h` |
| 2 | `TxnByDevice` | `Txn` | `device_id` | 1000 | 6 | 10000 | 480 | 480 | 480 | 1104 | 58192 | 58192 | 1584 | 58672 | 58672 | `cards_per_device_24h` |
| 3 | `TxnByMerchant` | `Txn` | `merchant_id` | 1000 | 4 | 10000 | 320 | 320 | 320 | 3096 | 31640 | 31640 | 3416 | 31960 | 31960 | `users_per_merchant_24h` |
| 4 | `TxnByCard` | `Txn` | `card_fp` | 1000 | 8 | 10000 | 640 | 640 | 640 | 2216 | 30760 | 30760 | 2856 | 31400 | 31400 | `merchants_per_card_24h` |
| 5 | `TxnByUser` | `Txn` | `user_id` | 9533 | 62 | 10000 | 4960 | 4960 | 4960 | 20494 | 20834 | 21434 | 25454 | 25794 | 26394 | `amount_p95_24h` |
| 6 | `CardAddByDevice` | `CardAdd` | `device_id` | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 7 | `LoginByUser` | `Login` | `user_id` | 0 | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 8 | `RefundByUser` | `Refund` | `user_id` | 0 | 8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |
| 9 | `SignupByIp` | `Signup` | `ip_address` | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | `-` |

## Per-Table Entity Details

### `TxnByIp` (`Txn` by `ip_address`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `cards_per_ip_1h` | `n_unique` | `windowed` | 80 | 424 | 28968 | 28968 | 504 | 29048 | 29048 |
| `users_per_ip_24h` | `n_unique` | `windowed` | 80 | 424 | 28968 | 28968 | 504 | 29048 | 29048 |
| `ip_top_users` | `top_k` | `windowed` | 80 | 1376 | 2144 | 2432 | 1456 | 2224 | 2512 |
| `amount_sum_per_ip_1h` | `sum` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_ip_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_ip_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `ip_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `ip_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s122` | 21 | 640 | 61136 | 61776 | `cards_per_ip_1h`=29048 bytes, `users_per_ip_24h`=29048 bytes, `ip_top_users`=2512 bytes |
| `s226` | 21 | 640 | 61136 | 61776 | `cards_per_ip_1h`=29048 bytes, `users_per_ip_24h`=29048 bytes, `ip_top_users`=2512 bytes |
| `s870` | 20 | 640 | 61040 | 61680 | `cards_per_ip_1h`=29048 bytes, `users_per_ip_24h`=29048 bytes, `ip_top_users`=2416 bytes |
| `s917` | 20 | 640 | 61040 | 61680 | `cards_per_ip_1h`=29048 bytes, `users_per_ip_24h`=29048 bytes, `ip_top_users`=2416 bytes |
| `s380` | 19 | 640 | 60944 | 61584 | `cards_per_ip_1h`=29048 bytes, `users_per_ip_24h`=29048 bytes, `ip_top_users`=2320 bytes |

#### Feature Breakdown For Largest Entity `s122`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `cards_per_ip_1h` | `n_unique` | `windowed` | 21 | 80 | 80 | 8 | 72 | 28968 | 29048 |
| `users_per_ip_24h` | `n_unique` | `windowed` | 21 | 80 | 80 | 8 | 72 | 28968 | 29048 |
| `ip_top_users` | `top_k` | `windowed` | 21 | 80 | 80 | 8 | 72 | 2432 | 2512 |
| `amount_sum_per_ip_1h` | `sum` | `windowed` | 21 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_ip_1h` | `count` | `windowed` | 21 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_ip_24h` | `count` | `windowed` | 21 | 80 | 80 | 8 | 72 | 256 | 336 |
| `ip_age` | `age` | `lifetime` | 21 | 80 | 80 | 32 | 48 | 0 | 80 |
| `ip_first_seen` | `first_seen` | `lifetime` | 21 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByDevice` (`Txn` by `device_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `cards_per_device_24h` | `n_unique` | `windowed` | 80 | 424 | 28968 | 28968 | 504 | 29048 | 29048 |
| `users_per_device_24h` | `n_unique` | `windowed` | 80 | 424 | 28968 | 28968 | 504 | 29048 | 29048 |
| `device_txn_count_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `device_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `device_last_seen` | `last_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s960` | 24 | 480 | 58192 | 58672 | `cards_per_device_24h`=29048 bytes, `users_per_device_24h`=29048 bytes, `device_txn_count_24h`=336 bytes |
| `s186` | 22 | 480 | 58192 | 58672 | `cards_per_device_24h`=29048 bytes, `users_per_device_24h`=29048 bytes, `device_txn_count_24h`=336 bytes |
| `s741` | 22 | 480 | 58192 | 58672 | `cards_per_device_24h`=29048 bytes, `users_per_device_24h`=29048 bytes, `device_txn_count_24h`=336 bytes |
| `s549` | 21 | 480 | 58192 | 58672 | `cards_per_device_24h`=29048 bytes, `users_per_device_24h`=29048 bytes, `device_txn_count_24h`=336 bytes |
| `s939` | 21 | 480 | 58192 | 58672 | `cards_per_device_24h`=29048 bytes, `users_per_device_24h`=29048 bytes, `device_txn_count_24h`=336 bytes |

#### Feature Breakdown For Largest Entity `s960`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `cards_per_device_24h` | `n_unique` | `windowed` | 24 | 80 | 80 | 8 | 72 | 28968 | 29048 |
| `users_per_device_24h` | `n_unique` | `windowed` | 24 | 80 | 80 | 8 | 72 | 28968 | 29048 |
| `device_txn_count_24h` | `count` | `windowed` | 24 | 80 | 80 | 8 | 72 | 256 | 336 |
| `device_age` | `age` | `lifetime` | 24 | 80 | 80 | 32 | 48 | 0 | 80 |
| `device_first_seen` | `first_seen` | `lifetime` | 24 | 80 | 80 | 32 | 48 | 0 | 80 |
| `device_last_seen` | `last_seen` | `lifetime` | 24 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByMerchant` (`Txn` by `merchant_id`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `users_per_merchant_24h` | `n_unique` | `windowed` | 80 | 424 | 28968 | 28968 | 504 | 29048 | 29048 |
| `merchant_amount_p99_24h` | `quantile` | `windowed` | 80 | 2416 | 2416 | 2416 | 2496 | 2496 | 2496 |
| `txn_per_merchant_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `merchant_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s201` | 23 | 320 | 31640 | 31960 | `users_per_merchant_24h`=29048 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s370` | 21 | 320 | 31640 | 31960 | `users_per_merchant_24h`=29048 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s494` | 21 | 320 | 31640 | 31960 | `users_per_merchant_24h`=29048 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s570` | 21 | 320 | 31640 | 31960 | `users_per_merchant_24h`=29048 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |
| `s591` | 20 | 320 | 31640 | 31960 | `users_per_merchant_24h`=29048 bytes, `merchant_amount_p99_24h`=2496 bytes, `txn_per_merchant_24h`=336 bytes |

#### Feature Breakdown For Largest Entity `s201`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `users_per_merchant_24h` | `n_unique` | `windowed` | 23 | 80 | 80 | 8 | 72 | 28968 | 29048 |
| `merchant_amount_p99_24h` | `quantile` | `windowed` | 23 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `txn_per_merchant_24h` | `count` | `windowed` | 23 | 80 | 80 | 8 | 72 | 256 | 336 |
| `merchant_first_seen` | `first_seen` | `lifetime` | 23 | 80 | 80 | 32 | 48 | 0 | 80 |

### `TxnByCard` (`Txn` by `card_fp`)

#### Feature Columns Across Entities

| Feature | Op | Shape | Stack bytes | Heap p50 | Heap p99 | Heap max | Total p50 | Total p99 | Total max |
|---------|----|-------|-------------|----------|----------|----------|-----------|-----------|-----------|
| `merchants_per_card_24h` | `n_unique` | `windowed` | 80 | 424 | 28968 | 28968 | 504 | 29048 | 29048 |
| `small_amt_burst_5m` | `burst_count` | `windowed` | 80 | 1024 | 1024 | 1024 | 1104 | 1104 | 1104 |
| `decline_count_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_card_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_per_card_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `card_age` | `age` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `card_first_seen` | `first_seen` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |
| `decline_streak_card` | `negative_streak` | `lifetime` | 80 | 0 | 0 | 0 | 80 | 80 | 80 |

#### Largest Entity Rows

| Entity key | Events | Stack bytes | Heap bytes | Total bytes | Top feature contributors |
|------------|--------|-------------|------------|-------------|--------------------------|
| `s774` | 26 | 640 | 30760 | 31400 | `merchants_per_card_24h`=29048 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=336 bytes |
| `s395` | 23 | 640 | 30760 | 31400 | `merchants_per_card_24h`=29048 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=336 bytes |
| `s546` | 23 | 640 | 30760 | 31400 | `merchants_per_card_24h`=29048 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=336 bytes |
| `s896` | 23 | 640 | 30760 | 31400 | `merchants_per_card_24h`=29048 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=336 bytes |
| `s505` | 21 | 640 | 30760 | 31400 | `merchants_per_card_24h`=29048 bytes, `small_amt_burst_5m`=1104 bytes, `decline_count_1h`=336 bytes |

#### Feature Breakdown For Largest Entity `s774`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `merchants_per_card_24h` | `n_unique` | `windowed` | 26 | 80 | 80 | 8 | 72 | 28968 | 29048 |
| `small_amt_burst_5m` | `burst_count` | `windowed` | 26 | 80 | 80 | 72 | 8 | 1024 | 1104 |
| `decline_count_1h` | `count` | `windowed` | 26 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_card_1h` | `count` | `windowed` | 26 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_per_card_24h` | `count` | `windowed` | 26 | 80 | 80 | 8 | 72 | 256 | 336 |
| `card_age` | `age` | `lifetime` | 26 | 80 | 80 | 32 | 48 | 0 | 80 |
| `card_first_seen` | `first_seen` | `lifetime` | 26 | 80 | 80 | 32 | 48 | 0 | 80 |
| `decline_streak_card` | `negative_streak` | `lifetime` | 26 | 80 | 80 | 8 | 72 | 0 | 80 |

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
| `top_merchants_24h` | `top_k` | `windowed` | 80 | 512 | 608 | 800 | 592 | 688 | 880 |
| `seasonal_dev` | `seasonal_deviation` | `lifetime` | 80 | 600 | 600 | 600 | 680 | 680 | 680 |
| `mcc_entropy_24h` | `entropy` | `windowed` | 80 | 396 | 480 | 648 | 476 | 560 | 728 |
| `countries_distinct_7d` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `ips_distinct_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `merchants_distinct_24h` | `n_unique` | `windowed` | 80 | 424 | 424 | 424 | 504 | 504 | 504 |
| `txn_count_5m` | `count` | `windowed` | 80 | 256 | 336 | 416 | 336 | 416 | 496 |
| `event_mix_24h` | `event_type_mix` | `lifetime` | 80 | 208 | 288 | 448 | 288 | 368 | 528 |
| `avg_amount_24h` | `mean` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `min_amount_24h` | `min` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `std_amount_24h` | `std` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `sum_amount_24h` | `sum` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_count_1h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `txn_count_24h` | `count` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `var_amount_24h` | `var` | `windowed` | 80 | 256 | 256 | 256 | 336 | 336 | 336 |
| `hour_hist_30d` | `hour_of_day_histogram` | `lifetime` | 80 | 192 | 192 | 192 | 272 | 272 | 272 |
| `unique_cells_24h` | `n_unique` | `lifetime` | 80 | 168 | 168 | 168 | 248 | 248 | 248 |
| `last_5_amounts` | `last_n` | `lifetime` | 80 | 160 | 160 | 160 | 240 | 240 | 240 |
| `recent_5_amts` | `most_recent_n` | `lifetime` | 80 | 160 | 160 | 160 | 240 | 240 | 240 |
| `first_5_merchants` | `first_n` | `lifetime` | 80 | 128 | 128 | 128 | 208 | 208 | 208 |
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
| `k00032049` | 4 | 4960 | 21434 | 26394 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00045411` | 3 | 4960 | 21174 | 26134 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00011780` | 3 | 4960 | 21094 | 26054 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00017976` | 3 | 4960 | 21094 | 26054 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |
| `k00035736` | 3 | 4960 | 21094 | 26054 | `amount_p95_24h`=2496 bytes, `p50_amount_24h`=2496 bytes, `p99_amount_24h`=2496 bytes |

#### Feature Breakdown For Largest Entity `k00032049`

| Feature | Op | Shape | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|---------|----|-------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|
| `amount_p95_24h` | `quantile` | `windowed` | 4 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `p50_amount_24h` | `quantile` | `windowed` | 4 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `p99_amount_24h` | `quantile` | `windowed` | 4 | 80 | 80 | 8 | 72 | 2416 | 2496 |
| `dist_from_home` | `distance_from_home` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 1720 | 1800 |
| `reservoir_50` | `reservoir_sample` | `lifetime` | 4 | 80 | 80 | 40 | 40 | 1600 | 1680 |
| `dow_hour_hist_30d` | `dow_hour_histogram` | `lifetime` | 4 | 80 | 80 | 24 | 56 | 1344 | 1424 |
| `device_seen` | `bloom_member` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 1280 | 1360 |
| `burst_count_5m` | `burst_count` | `windowed` | 4 | 80 | 80 | 72 | 8 | 1024 | 1104 |
| `top_merchants_24h` | `top_k` | `windowed` | 4 | 80 | 80 | 8 | 72 | 800 | 880 |
| `mcc_entropy_24h` | `entropy` | `windowed` | 4 | 80 | 80 | 8 | 72 | 648 | 728 |
| `seasonal_dev` | `seasonal_deviation` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 600 | 680 |
| `event_mix_24h` | `event_type_mix` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 448 | 528 |
| `countries_distinct_7d` | `n_unique` | `windowed` | 4 | 80 | 80 | 8 | 72 | 424 | 504 |
| `ips_distinct_24h` | `n_unique` | `windowed` | 4 | 80 | 80 | 8 | 72 | 424 | 504 |
| `merchants_distinct_24h` | `n_unique` | `windowed` | 4 | 80 | 80 | 8 | 72 | 424 | 504 |
| `txn_count_5m` | `count` | `windowed` | 4 | 80 | 80 | 8 | 72 | 416 | 496 |
| `avg_amount_24h` | `mean` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `min_amount_24h` | `min` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `std_amount_24h` | `std` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `sum_amount_24h` | `sum` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_count_1h` | `count` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `txn_count_24h` | `count` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `var_amount_24h` | `var` | `windowed` | 4 | 80 | 80 | 8 | 72 | 256 | 336 |
| `hour_hist_30d` | `hour_of_day_histogram` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 192 | 272 |
| `unique_cells_24h` | `n_unique` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 168 | 248 |
| `last_5_amounts` | `last_n` | `lifetime` | 4 | 80 | 80 | 40 | 40 | 160 | 240 |
| `recent_5_amts` | `most_recent_n` | `lifetime` | 4 | 80 | 80 | 48 | 32 | 160 | 240 |
| `first_5_merchants` | `first_n` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 128 | 208 |
| `geo_kmh` | `geo_velocity` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 94 | 174 |
| `geo_spread_24h` | `geo_spread` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 94 | 174 |
| `geo_dist_last` | `geo_distance` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 86 | 166 |
| `amount_lag1` | `lag` | `lifetime` | 4 | 80 | 80 | 40 | 40 | 64 | 144 |
| `geo_entropy_24h` | `entropy` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 56 | 136 |
| `time_since_last_5` | `time_since_last_n` | `lifetime` | 4 | 80 | 80 | 40 | 40 | 40 | 120 |
| `age` | `age` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_decayed_sum_24h` | `decayed_sum` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_delta` | `delta_from_prev` | `lifetime` | 4 | 80 | 80 | 24 | 56 | 0 | 80 |
| `amount_ew_zscore` | `ew_zscore` | `lifetime` | 4 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_ewma_1h` | `ewma` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_ewvar_1h` | `ewvar` | `lifetime` | 4 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_outliers_5m` | `outlier_count` | `windowed` | 4 | 80 | 80 | 40 | 40 | 0 | 80 |
| `amount_rate_5m` | `rate_of_change` | `windowed` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `amount_trend_5m` | `trend` | `windowed` | 4 | 80 | 80 | 48 | 32 | 0 | 80 |
| `amount_trend_resid_5m` | `trend_residual` | `windowed` | 4 | 80 | 80 | 72 | 8 | 0 | 80 |
| `amount_twa_5m` | `twa` | `windowed` | 4 | 80 | 80 | 40 | 40 | 0 | 80 |
| `amount_z_score` | `z_score` | `windowed` | 4 | 80 | 80 | 40 | 40 | 0 | 80 |
| `decline_streak` | `negative_streak` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 0 | 80 |
| `device_change_count_5m` | `value_change_count` | `windowed` | 4 | 80 | 80 | 40 | 40 | 0 | 80 |
| `first_amount` | `first` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `first_in_24h` | `first_seen_in_window` | `windowed` | 4 | 80 | 80 | 24 | 56 | 0 | 80 |
| `first_seen` | `first_seen` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `has_seen` | `has_seen` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `inter_arrival_1h` | `inter_arrival_stats` | `windowed` | 4 | 80 | 80 | 40 | 40 | 0 | 80 |
| `last_amount` | `last` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `last_seen` | `last_seen` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `max_amount_lifetime` | `max` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `max_streak` | `max_streak` | `lifetime` | 4 | 80 | 80 | 16 | 64 | 0 | 80 |
| `sum_amount_lifetime` | `sum` | `lifetime` | 4 | 80 | 80 | 16 | 64 | 0 | 80 |
| `time_since_last` | `time_since` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `txn_count_lifetime` | `count` | `lifetime` | 4 | 80 | 80 | 8 | 72 | 0 | 80 |
| `txn_decayed_count_24h` | `decayed_count` | `lifetime` | 4 | 80 | 80 | 32 | 48 | 0 | 80 |
| `txn_streak` | `streak` | `lifetime` | 4 | 80 | 80 | 16 | 64 | 0 | 80 |

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

### 1. `Txn` / `TxnByCard` / `merchants_per_card_24h` / `n_unique`

- Path: `Txn` -> `TxnByCard` -> `merchants_per_card_24h` -> `n_unique` -> `windowed`
- Entity key: `s774`
- Entity events: `26`
- Key path: `card_fp`
- Events applied: `26`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=28968 total=29048
- Shape: `windowed` (1d)
- Breakdown rollup:
  - `CountDistinct hash-set slots across buckets`: 28672 bytes (HashSet, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Box<CountDistinctStateWrap> across buckets`: 40 bytes (Box, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 0 / CountDistinct hash-set slots`: 28672 bytes (HashSet, estimated hashbrown slot cost for u64 distinct hashes)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<CountDistinctStateWrap>`: 40 bytes (Box, heap allocation for boxed CountDistinct wrapper)

### 2. `Txn` / `TxnByIp` / `ip_top_users` / `top_k`

- Path: `Txn` -> `TxnByIp` -> `ip_top_users` -> `top_k` -> `windowed`
- Entity key: `s122`
- Entity events: `21`
- Key path: `ip_address`
- Events applied: `21`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=2432 total=2512
- Shape: `windowed` (1d)
- Breakdown rollup:
  - `TopK exact BTreeMap entries across buckets`: 2016 bytes (BTreeMap, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<TopKStateWrap> across buckets`: 160 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / TopK exact BTreeMap entries`: 2016 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 3. `Txn` / `TxnByMerchant` / `merchant_amount_p99_24h` / `quantile`

- Path: `Txn` -> `TxnByMerchant` -> `merchant_amount_p99_24h` -> `quantile` -> `windowed`
- Entity key: `s201`
- Entity events: `23`
- Key path: `merchant_id`
- Events applied: `23`
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
- Entity key: `k00032049`
- Entity events: `4`
- Key path: `user_id`
- Events applied: `4`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=1720 total=1800
- Shape: `lifetime`
- Breakdown:
  - `DistanceFromHome coordinate buffer`: 1600 bytes (Vec, capacity * size_of::<(f64, f64)>())
  - `Box<DistanceFromHomeState>`: 120 bytes (Box, heap allocation for boxed payload)

### 5. `Txn` / `TxnByUser` / `reservoir_50` / `reservoir_sample`

- Path: `Txn` -> `TxnByUser` -> `reservoir_50` -> `reservoir_sample` -> `lifetime`
- Entity key: `k00032049`
- Entity events: `4`
- Key path: `user_id`
- Events applied: `4`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=40 slack_bytes=40) heap=1600 total=1680
- Shape: `lifetime`
- Breakdown:
  - `ReservoirSample reservoir`: 1600 bytes (Vec, capacity * size_of::<Value>())

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile bytes-per-active-entity-row p99: `25794` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 18794 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- `enum_slot_bytes` is the fixed-size `AggOp` enum slot charged to a row; parent rows sum this across child paths.
- `payload_bytes` is the active variant payload inside the enum slot. For boxed variants this is the inline `Box<T>` pointer, while the boxed pointee remains in `heap_bytes`.
- `slack_bytes` is unused capacity in the fixed-size `AggOp` enum slot: `enum_slot_bytes - payload_bytes`.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
- Primary grain is `derivation table -> entity row -> feature column`; top offenders list one concrete entity-feature row per unique op.
