# AggOp Memory Profile: fraud-team

## Workload Summary

- Workload: `fraud`
- Events requested from generator: `2000`
- Events replayed from generator: `2000`
- Events by source:
  - `Txn`: `2000`
- Derivations discovered: `9`
- Aggregate features discovered: `111`
- Per-entity structural estimate: `474904` bytes

## Sorted Op Table

| Rank | Op | Shape | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes |
|------|----|-------|-------------|-----------------|---------------|-------------|------------|-------------|
| 1 | `n_unique` | `windowed` | 1040 | 1040 | 104 | 936 | 187688 | 188728 |
| 2 | `top_k` | `windowed` | 160 | 160 | 16 | 144 | 149704 | 149864 |
| 3 | `entropy` | `windowed` | 80 | 80 | 8 | 72 | 72623 | 72703 |
| 4 | `event_type_mix` | `lifetime` | 80 | 80 | 8 | 72 | 20608 | 20688 |
| 5 | `quantile` | `windowed` | 320 | 320 | 32 | 288 | 17856 | 18176 |
| 6 | `count` | `windowed` | 1200 | 1200 | 120 | 1080 | 3440 | 4640 |
| 7 | `burst_count` | `windowed` | 240 | 240 | 216 | 24 | 3072 | 3312 |
| 8 | `distance_from_home` | `lifetime` | 80 | 80 | 8 | 72 | 1720 | 1800 |
| 9 | `reservoir_sample` | `lifetime` | 80 | 80 | 40 | 40 | 1600 | 1680 |
| 10 | `seasonal_deviation` | `lifetime` | 80 | 80 | 8 | 72 | 1427 | 1507 |
| 11 | `dow_hour_histogram` | `lifetime` | 80 | 80 | 24 | 56 | 1344 | 1424 |
| 12 | `bloom_member` | `lifetime` | 80 | 80 | 8 | 72 | 1280 | 1360 |
| 13 | `sum` | `windowed` | 160 | 160 | 16 | 144 | 512 | 672 |
| 14 | `geo_velocity` | `lifetime` | 160 | 160 | 16 | 144 | 357 | 517 |
| 15 | `n_unique` | `lifetime` | 160 | 160 | 16 | 144 | 336 | 496 |
| 16 | `first_seen` | `lifetime` | 400 | 400 | 160 | 240 | 0 | 400 |
| 17 | `first_n` | `lifetime` | 80 | 80 | 32 | 48 | 256 | 336 |
| 18 | `mean` | `windowed` | 80 | 80 | 8 | 72 | 256 | 336 |
| 19 | `min` | `windowed` | 80 | 80 | 8 | 72 | 256 | 336 |
| 20 | `std` | `windowed` | 80 | 80 | 8 | 72 | 256 | 336 |
| 21 | `var` | `windowed` | 80 | 80 | 8 | 72 | 256 | 336 |
| 22 | `hour_of_day_histogram` | `lifetime` | 80 | 80 | 8 | 72 | 255 | 335 |
| 23 | `geo_spread` | `lifetime` | 80 | 80 | 8 | 72 | 250 | 330 |
| 24 | `age` | `lifetime` | 320 | 320 | 128 | 192 | 0 | 320 |
| 25 | `negative_streak` | `lifetime` | 320 | 320 | 32 | 288 | 0 | 320 |
| 26 | `geo_distance` | `lifetime` | 80 | 80 | 8 | 72 | 192 | 272 |
| 27 | `count` | `lifetime` | 240 | 240 | 24 | 216 | 0 | 240 |
| 28 | `last_n` | `lifetime` | 80 | 80 | 40 | 40 | 160 | 240 |
| 29 | `last_seen` | `lifetime` | 240 | 240 | 96 | 144 | 0 | 240 |
| 30 | `most_recent_n` | `lifetime` | 80 | 80 | 48 | 32 | 160 | 240 |
| 31 | `time_since` | `lifetime` | 240 | 240 | 96 | 144 | 0 | 240 |
| 32 | `decayed_count` | `lifetime` | 160 | 160 | 64 | 96 | 0 | 160 |
| 33 | `first_seen_in_window` | `windowed` | 160 | 160 | 48 | 112 | 0 | 160 |
| 34 | `streak` | `lifetime` | 160 | 160 | 32 | 128 | 0 | 160 |
| 35 | `sum` | `lifetime` | 160 | 160 | 32 | 128 | 0 | 160 |
| 36 | `lag` | `lifetime` | 80 | 80 | 40 | 40 | 64 | 144 |
| 37 | `entropy` | `lifetime` | 80 | 80 | 8 | 72 | 56 | 136 |
| 38 | `time_since_last_n` | `lifetime` | 80 | 80 | 40 | 40 | 40 | 120 |
| 39 | `decayed_sum` | `lifetime` | 80 | 80 | 32 | 48 | 0 | 80 |
| 40 | `delta_from_prev` | `lifetime` | 80 | 80 | 24 | 56 | 0 | 80 |
| 41 | `ew_zscore` | `lifetime` | 80 | 80 | 48 | 32 | 0 | 80 |
| 42 | `ewma` | `lifetime` | 80 | 80 | 32 | 48 | 0 | 80 |
| 43 | `ewvar` | `lifetime` | 80 | 80 | 48 | 32 | 0 | 80 |
| 44 | `first` | `lifetime` | 80 | 80 | 32 | 48 | 0 | 80 |
| 45 | `has_seen` | `lifetime` | 80 | 80 | 32 | 48 | 0 | 80 |
| 46 | `inter_arrival_stats` | `windowed` | 80 | 80 | 40 | 40 | 0 | 80 |
| 47 | `last` | `lifetime` | 80 | 80 | 32 | 48 | 0 | 80 |
| 48 | `max` | `lifetime` | 80 | 80 | 32 | 48 | 0 | 80 |
| 49 | `max_streak` | `lifetime` | 80 | 80 | 16 | 64 | 0 | 80 |
| 50 | `outlier_count` | `windowed` | 80 | 80 | 40 | 40 | 0 | 80 |
| 51 | `rate_of_change` | `windowed` | 80 | 80 | 32 | 48 | 0 | 80 |
| 52 | `trend` | `windowed` | 80 | 80 | 48 | 32 | 0 | 80 |
| 53 | `trend_residual` | `windowed` | 80 | 80 | 72 | 8 | 0 | 80 |
| 54 | `twa` | `windowed` | 80 | 80 | 40 | 40 | 0 | 80 |
| 55 | `value_change_count` | `windowed` | 80 | 80 | 40 | 40 | 0 | 80 |
| 56 | `z_score` | `windowed` | 80 | 80 | 40 | 40 | 0 | 80 |

## Sorted Op Path Details

Rows with `0` events applied show constructor footprint only; the workload generator did not emit an event for that path's upstream source.

### 1. `n_unique` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 1 | `Txn` | `TxnByCard` | `merchants_per_card_24h` | `card_fp` | 2000 | 80 | 80 | 8 | 72 | 28968 | 29048 | 15.4% |
| 1 | `Txn` | `TxnByDevice` | `cards_per_device_24h` | `device_id` | 2000 | 80 | 80 | 8 | 72 | 28968 | 29048 | 15.4% |
| 1 | `Txn` | `TxnByIp` | `cards_per_ip_1h` | `ip_address` | 2000 | 80 | 80 | 8 | 72 | 28968 | 29048 | 15.4% |
| 1 | `Txn` | `TxnByUser` | `countries_distinct_7d` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 28968 | 29048 | 15.4% |
| 1 | `Txn` | `TxnByUser` | `ips_distinct_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 28968 | 29048 | 15.4% |
| 1 | `Txn` | `TxnByUser` | `merchants_distinct_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 28968 | 29048 | 15.4% |
| 1 | `Txn` | `TxnByDevice` | `users_per_device_24h` | `device_id` | 2000 | 80 | 80 | 8 | 72 | 4392 | 4472 | 2.4% |
| 1 | `Txn` | `TxnByIp` | `users_per_ip_24h` | `ip_address` | 2000 | 80 | 80 | 8 | 72 | 4392 | 4472 | 2.4% |
| 1 | `Txn` | `TxnByMerchant` | `users_per_merchant_24h` | `merchant_id` | 2000 | 80 | 80 | 8 | 72 | 4392 | 4472 | 2.4% |
| 1 | `Login` | `LoginByUser` | `ips_distinct_login_1h` | `user_id` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 0.1% |
| 1 | `Login` | `LoginByUser` | `uas_distinct_login_24h` | `user_id` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 0.1% |
| 1 | `Signup` | `SignupByIp` | `emails_per_ip_24h` | `ip_address` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 0.1% |
| 1 | `Signup` | `SignupByIp` | `ssn_reuse_per_ip_30d` | `ip_address` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 0.1% |

### 2. `top_k` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 2 | `Txn` | `TxnByUser` | `top_merchants_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 83264 | 83344 | 55.6% |
| 2 | `Txn` | `TxnByIp` | `ip_top_users` | `ip_address` | 2000 | 80 | 80 | 8 | 72 | 66440 | 66520 | 44.4% |

### 3. `entropy` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 3 | `Txn` | `TxnByUser` | `mcc_entropy_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 72623 | 72703 | 100.0% |

### 4. `event_type_mix` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 4 | `Txn` | `TxnByUser` | `event_mix_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 20608 | 20688 | 100.0% |

### 5. `quantile` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 5 | `Txn` | `TxnByMerchant` | `merchant_amount_p99_24h` | `merchant_id` | 2000 | 80 | 80 | 8 | 72 | 4464 | 4544 | 25.0% |
| 5 | `Txn` | `TxnByUser` | `amount_p95_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 4464 | 4544 | 25.0% |
| 5 | `Txn` | `TxnByUser` | `p50_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 4464 | 4544 | 25.0% |
| 5 | `Txn` | `TxnByUser` | `p99_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 4464 | 4544 | 25.0% |

### 6. `count` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 6 | `Txn` | `TxnByCard` | `decline_count_1h` | `card_fp` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByCard` | `txn_per_card_1h` | `card_fp` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByCard` | `txn_per_card_24h` | `card_fp` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByDevice` | `device_txn_count_24h` | `device_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByIp` | `txn_per_ip_1h` | `ip_address` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByIp` | `txn_per_ip_24h` | `ip_address` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByMerchant` | `txn_per_merchant_24h` | `merchant_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByUser` | `txn_count_1h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByUser` | `txn_count_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `Txn` | `TxnByUser` | `txn_count_5m` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 7.2% |
| 6 | `CardAdd` | `CardAddByDevice` | `card_add_per_device_24h` | `device_id` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 5.5% |
| 6 | `Login` | `LoginByUser` | `login_count_1h` | `user_id` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 5.5% |
| 6 | `Login` | `LoginByUser` | `login_count_24h` | `user_id` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 5.5% |
| 6 | `Refund` | `RefundByUser` | `refund_count_24h` | `user_id` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 5.5% |
| 6 | `Signup` | `SignupByIp` | `signup_per_ip_24h` | `ip_address` | 0 | 80 | 80 | 8 | 72 | 176 | 256 | 5.5% |

### 7. `burst_count` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 7 | `Txn` | `TxnByCard` | `small_amt_burst_5m` | `card_fp` | 2000 | 80 | 80 | 72 | 8 | 1024 | 1104 | 33.3% |
| 7 | `Txn` | `TxnByUser` | `burst_count_5m` | `user_id` | 2000 | 80 | 80 | 72 | 8 | 1024 | 1104 | 33.3% |
| 7 | `Signup` | `SignupByIp` | `signup_burst_10m` | `ip_address` | 0 | 80 | 80 | 72 | 8 | 1024 | 1104 | 33.3% |

### 8. `distance_from_home` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 8 | `Txn` | `TxnByUser` | `dist_from_home` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 1720 | 1800 | 100.0% |

### 9. `reservoir_sample` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 9 | `Txn` | `TxnByUser` | `reservoir_50` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 1600 | 1680 | 100.0% |

### 10. `seasonal_deviation` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 10 | `Txn` | `TxnByUser` | `seasonal_dev` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 1427 | 1507 | 100.0% |

### 11. `dow_hour_histogram` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 11 | `Txn` | `TxnByUser` | `dow_hour_hist_30d` | `user_id` | 2000 | 80 | 80 | 24 | 56 | 1344 | 1424 | 100.0% |

### 12. `bloom_member` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 12 | `Txn` | `TxnByUser` | `device_seen` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 1280 | 1360 | 100.0% |

### 13. `sum` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 13 | `Txn` | `TxnByIp` | `amount_sum_per_ip_1h` | `ip_address` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 50.0% |
| 13 | `Txn` | `TxnByUser` | `sum_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 50.0% |

### 14. `geo_velocity` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 14 | `Txn` | `TxnByUser` | `geo_kmh` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 206 | 286 | 55.3% |
| 14 | `Login` | `LoginByUser` | `login_geo_kmh` | `user_id` | 0 | 80 | 80 | 8 | 72 | 151 | 231 | 44.7% |

### 15. `n_unique` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 15 | `Txn` | `TxnByUser` | `unique_cells_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 168 | 248 | 50.0% |
| 15 | `CardAdd` | `CardAddByDevice` | `cards_per_device_lifetime` | `device_id` | 0 | 80 | 80 | 8 | 72 | 168 | 248 | 50.0% |

### 16. `first_seen` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 16 | `Txn` | `TxnByCard` | `card_first_seen` | `card_fp` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 20.0% |
| 16 | `Txn` | `TxnByDevice` | `device_first_seen` | `device_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 20.0% |
| 16 | `Txn` | `TxnByIp` | `ip_first_seen` | `ip_address` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 20.0% |
| 16 | `Txn` | `TxnByMerchant` | `merchant_first_seen` | `merchant_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 20.0% |
| 16 | `Txn` | `TxnByUser` | `first_seen` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 20.0% |

### 17. `first_n` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 17 | `Txn` | `TxnByUser` | `first_5_merchants` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 256 | 336 | 100.0% |

### 18. `mean` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 18 | `Txn` | `TxnByUser` | `avg_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 100.0% |

### 19. `min` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 19 | `Txn` | `TxnByUser` | `min_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 100.0% |

### 20. `std` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 20 | `Txn` | `TxnByUser` | `std_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 100.0% |

### 21. `var` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 21 | `Txn` | `TxnByUser` | `var_amount_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 256 | 336 | 100.0% |

### 22. `hour_of_day_histogram` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 22 | `Txn` | `TxnByUser` | `hour_hist_30d` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 255 | 335 | 100.0% |

### 23. `geo_spread` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 23 | `Txn` | `TxnByUser` | `geo_spread_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 250 | 330 | 100.0% |

### 24. `age` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 24 | `Txn` | `TxnByCard` | `card_age` | `card_fp` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 25.0% |
| 24 | `Txn` | `TxnByDevice` | `device_age` | `device_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 25.0% |
| 24 | `Txn` | `TxnByIp` | `ip_age` | `ip_address` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 25.0% |
| 24 | `Txn` | `TxnByUser` | `age` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 25.0% |

### 25. `negative_streak` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 25 | `Txn` | `TxnByCard` | `decline_streak_card` | `card_fp` | 2000 | 80 | 80 | 8 | 72 | 0 | 80 | 25.0% |
| 25 | `Txn` | `TxnByUser` | `decline_streak` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 0 | 80 | 25.0% |
| 25 | `CardAdd` | `CardAddByDevice` | `card_add_failure_streak` | `device_id` | 0 | 80 | 80 | 8 | 72 | 0 | 80 | 25.0% |
| 25 | `Login` | `LoginByUser` | `failed_login_streak` | `user_id` | 0 | 80 | 80 | 8 | 72 | 0 | 80 | 25.0% |

### 26. `geo_distance` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 26 | `Txn` | `TxnByUser` | `geo_dist_last` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 192 | 272 | 100.0% |

### 27. `count` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 27 | `Txn` | `TxnByUser` | `txn_count_lifetime` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 0 | 80 | 33.3% |
| 27 | `Refund` | `RefundByUser` | `chargeback_count_lifetime` | `user_id` | 0 | 80 | 80 | 8 | 72 | 0 | 80 | 33.3% |
| 27 | `Refund` | `RefundByUser` | `refund_count_lifetime` | `user_id` | 0 | 80 | 80 | 8 | 72 | 0 | 80 | 33.3% |

### 28. `last_n` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 28 | `Txn` | `TxnByUser` | `last_5_amounts` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 160 | 240 | 100.0% |

### 29. `last_seen` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 29 | `Txn` | `TxnByDevice` | `device_last_seen` | `device_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 33.3% |
| 29 | `Txn` | `TxnByUser` | `last_seen` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 33.3% |
| 29 | `Login` | `LoginByUser` | `last_login_at` | `user_id` | 0 | 80 | 80 | 32 | 48 | 0 | 80 | 33.3% |

### 30. `most_recent_n` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 30 | `Txn` | `TxnByUser` | `recent_5_amts` | `user_id` | 2000 | 80 | 80 | 48 | 32 | 160 | 240 | 100.0% |

### 31. `time_since` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 31 | `Txn` | `TxnByUser` | `time_since_last` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 33.3% |
| 31 | `Login` | `LoginByUser` | `time_since_last_login` | `user_id` | 0 | 80 | 80 | 32 | 48 | 0 | 80 | 33.3% |
| 31 | `Refund` | `RefundByUser` | `time_since_last_cb` | `user_id` | 0 | 80 | 80 | 32 | 48 | 0 | 80 | 33.3% |

### 32. `decayed_count` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 32 | `Txn` | `TxnByUser` | `txn_decayed_count_24h` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 50.0% |
| 32 | `Refund` | `RefundByUser` | `chargeback_decayed_count` | `user_id` | 0 | 80 | 80 | 32 | 48 | 0 | 80 | 50.0% |

### 33. `first_seen_in_window` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 33 | `Txn` | `TxnByUser` | `first_in_24h` | `user_id` | 2000 | 80 | 80 | 24 | 56 | 0 | 80 | 50.0% |
| 33 | `Refund` | `RefundByUser` | `first_refund_in_30d` | `user_id` | 0 | 80 | 80 | 24 | 56 | 0 | 80 | 50.0% |

### 34. `streak` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 34 | `Txn` | `TxnByUser` | `txn_streak` | `user_id` | 2000 | 80 | 80 | 16 | 64 | 0 | 80 | 50.0% |
| 34 | `Refund` | `RefundByUser` | `cb_streak` | `user_id` | 0 | 80 | 80 | 16 | 64 | 0 | 80 | 50.0% |

### 35. `sum` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 35 | `Txn` | `TxnByUser` | `sum_amount_lifetime` | `user_id` | 2000 | 80 | 80 | 16 | 64 | 0 | 80 | 50.0% |
| 35 | `Refund` | `RefundByUser` | `refund_amount_lifetime` | `user_id` | 0 | 80 | 80 | 16 | 64 | 0 | 80 | 50.0% |

### 36. `lag` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 36 | `Txn` | `TxnByUser` | `amount_lag1` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 64 | 144 | 100.0% |

### 37. `entropy` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 37 | `Txn` | `TxnByUser` | `geo_entropy_24h` | `user_id` | 2000 | 80 | 80 | 8 | 72 | 56 | 136 | 100.0% |

### 38. `time_since_last_n` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 38 | `Txn` | `TxnByUser` | `time_since_last_5` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 40 | 120 | 100.0% |

### 39. `decayed_sum` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 39 | `Txn` | `TxnByUser` | `amount_decayed_sum_24h` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 40. `delta_from_prev` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 40 | `Txn` | `TxnByUser` | `amount_delta` | `user_id` | 2000 | 80 | 80 | 24 | 56 | 0 | 80 | 100.0% |

### 41. `ew_zscore` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 41 | `Txn` | `TxnByUser` | `amount_ew_zscore` | `user_id` | 2000 | 80 | 80 | 48 | 32 | 0 | 80 | 100.0% |

### 42. `ewma` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 42 | `Txn` | `TxnByUser` | `amount_ewma_1h` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 43. `ewvar` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 43 | `Txn` | `TxnByUser` | `amount_ewvar_1h` | `user_id` | 2000 | 80 | 80 | 48 | 32 | 0 | 80 | 100.0% |

### 44. `first` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 44 | `Txn` | `TxnByUser` | `first_amount` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 45. `has_seen` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 45 | `Txn` | `TxnByUser` | `has_seen` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 46. `inter_arrival_stats` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 46 | `Txn` | `TxnByUser` | `inter_arrival_1h` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 0 | 80 | 100.0% |

### 47. `last` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 47 | `Txn` | `TxnByUser` | `last_amount` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 48. `max` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 48 | `Txn` | `TxnByUser` | `max_amount_lifetime` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 49. `max_streak` / `lifetime` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 49 | `Txn` | `TxnByUser` | `max_streak` | `user_id` | 2000 | 80 | 80 | 16 | 64 | 0 | 80 | 100.0% |

### 50. `outlier_count` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 50 | `Txn` | `TxnByUser` | `amount_outliers_5m` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 0 | 80 | 100.0% |

### 51. `rate_of_change` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 51 | `Txn` | `TxnByUser` | `amount_rate_5m` | `user_id` | 2000 | 80 | 80 | 32 | 48 | 0 | 80 | 100.0% |

### 52. `trend` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 52 | `Txn` | `TxnByUser` | `amount_trend_5m` | `user_id` | 2000 | 80 | 80 | 48 | 32 | 0 | 80 | 100.0% |

### 53. `trend_residual` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 53 | `Txn` | `TxnByUser` | `amount_trend_resid_5m` | `user_id` | 2000 | 80 | 80 | 72 | 8 | 0 | 80 | 100.0% |

### 54. `twa` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 54 | `Txn` | `TxnByUser` | `amount_twa_5m` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 0 | 80 | 100.0% |

### 55. `value_change_count` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 55 | `Txn` | `TxnByUser` | `device_change_count_5m` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 0 | 80 | 100.0% |

### 56. `z_score` / `windowed` paths

| Parent rank | Source event | Derivation | Feature | Key path | Events applied | Stack bytes | enum_slot_bytes | payload_bytes | slack_bytes | Heap bytes | Total bytes | Parent % |
|-------------|--------------|------------|---------|----------|----------------|-------------|-----------------|---------------|-------------|------------|-------------|----------|
| 56 | `Txn` | `TxnByUser` | `amount_z_score` | `user_id` | 2000 | 80 | 80 | 40 | 40 | 0 | 80 | 100.0% |

## Top 5 Offenders

### 1. `Txn` / `TxnByUser` / `top_merchants_24h` / `top_k`

- Path: `Txn` -> `TxnByUser` -> `top_merchants_24h` -> `top_k` -> `windowed`
- Key path: `user_id`
- Events applied: `2000`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=83264 total=83344
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `TopK exact BTreeMap entries across buckets`: 82848 bytes (BTreeMap, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<TopKStateWrap> across buckets`: 160 bytes (Box, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / TopK exact BTreeMap entries`: 82848 bytes (BTreeMap, estimated node overhead plus TopKValue/u64 payloads)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 2. `Txn` / `TxnByUser` / `mcc_entropy_24h` / `entropy`

- Path: `Txn` -> `TxnByUser` -> `mcc_entropy_24h` -> `entropy` -> `windowed`
- Key path: `user_id`
- Events applied: `2000`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=72623 total=72703
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `Entropy category map entries across buckets`: 68960 bytes (BTreeMap, summed across active window buckets)
  - `Entropy category string capacity across buckets`: 3351 bytes (String, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
  - `Box<EntropyStateWrap> across buckets`: 56 bytes (Box, summed across active window buckets)
- Raw breakdown:
  - `Windowed bucket 0 / Entropy category map entries`: 68960 bytes (BTreeMap, estimated node overhead plus String/u64 category payloads)
  - `Windowed bucket 0 / Entropy category string capacity`: 3351 bytes (String, sum of tracked category string capacities)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)
  - `Windowed bucket 0 / Box<EntropyStateWrap>`: 56 bytes (Box, heap allocation for boxed Entropy wrapper)

### 3. `Txn` / `TxnByIp` / `ip_top_users` / `top_k`

- Path: `Txn` -> `TxnByIp` -> `ip_top_users` -> `top_k` -> `windowed`
- Key path: `ip_address`
- Events applied: `2000`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=66440 total=66520
- Shape: `windowed` (1d)
- Recommendation: restructure only if lazy bucket materialization still dominates
- Breakdown rollup:
  - `TopK count-min counters across buckets`: 65536 bytes (Vec, summed across active window buckets)
  - `TopK heap index map across buckets`: 392 bytes (AHashMap, summed across active window buckets)
  - `Windowed wrapper overhead`: 176 bytes (WindowedOp, summed boxed WindowedOp payload and spilled bucket storage)
  - `Box<TopKStateWrap> across buckets`: 160 bytes (Box, summed across active window buckets)
  - `TopK heap entries across buckets`: 96 bytes (Vec, summed across active window buckets)
  - `Windowed bucket shell overhead`: 80 bytes (Box, summed boxed AggOp enum slots across active buckets)
- Raw breakdown:
  - `Windowed bucket 0 / TopK count-min counters`: 65536 bytes (Vec, capacity * size_of::<i64>() for count-min sketch counters)
  - `Windowed bucket 0 / TopK heap index map`: 392 bytes (AHashMap, estimated slot cost for TopK heap-position side index)
  - `Box<WindowedOp>`: 176 bytes (Box, heap allocation for boxed WindowedOp payload)
  - `Windowed bucket 0 / Box<TopKStateWrap>`: 160 bytes (Box, heap allocation for boxed TopK wrapper)
  - `Windowed bucket 0 / TopK heap entries`: 96 bytes (Vec, capacity * size_of::<(u64, TopKValue)>() for bounded top-k heap entries)
  - `Windowed bucket 0 Box<AggOp>`: 80 bytes (Box, heap allocation for bucket AggOp enum slot)

### 4. `Txn` / `TxnByCard` / `merchants_per_card_24h` / `n_unique`

- Path: `Txn` -> `TxnByCard` -> `merchants_per_card_24h` -> `n_unique` -> `windowed`
- Key path: `card_fp`
- Events applied: `2000`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=28968 total=29048
- Shape: `windowed` (1d)
- Recommendation: keep for now; quantify sketch precision and window bucket fanout separately
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

### 5. `Txn` / `TxnByDevice` / `cards_per_device_24h` / `n_unique`

- Path: `Txn` -> `TxnByDevice` -> `cards_per_device_24h` -> `n_unique` -> `windowed`
- Key path: `device_id`
- Events applied: `2000`
- Bytes: stack=80 (enum_slot_bytes=80 payload_bytes=8 slack_bytes=72) heap=28968 total=29048
- Shape: `windowed` (1d)
- Recommendation: keep for now; quantify sketch precision and window bucket fanout separately
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

## Metrics Coherence

- `/metrics` `beava_bytes_per_entity_p99`: `7000` bytes
- Profile per-entity estimate: `474904` bytes
- Tolerance: `15.0%`
- Assertion: bytes_per_entity_p99 diverged by 467904 bytes; file sibling work to replace the static placeholder with live sampling.

## Notes

- `stack_bytes` is the inline `AggOp` enum slot for each feature.
- `enum_slot_bytes` is the fixed-size `AggOp` enum slot charged to a row; parent rows sum this across child paths.
- `payload_bytes` is the active variant payload inside the enum slot. For boxed variants this is the inline `Box<T>` pointer, while the boxed pointee remains in `heap_bytes`.
- `slack_bytes` is unused capacity in the fixed-size `AggOp` enum slot: `enum_slot_bytes - payload_bytes`.
- Heap entries are deterministic structural counts; map/table allocator overhead is labeled as an estimate.
- Path grain is `source_event -> derivation -> feature -> op -> shape`; path detail rows are children of the op/shape rollups above.
