# Beava use cases — research report

Synthesised from `beava-use-cases/results/` (38 per-item JSON files, one per candidate use-case domain). Each section links back to the per-item JSON for the full field-by-field detail; this report distils the load-bearing findings.

**Scope.** beava as an open-source, single-binary, real-time feature server (Rust + Python SDK, Apache-2.0). Each candidate domain was scored on six categories: domain fit, technical fit, commercial fit, competition, adoption signals, and risk / non-goals.

**Caveat on uncertain fields.** Every per-item JSON carries a small `uncertain` array. Six recurring uncertains are *not* per-item gaps — they're structural: per-tenant `throughput_profile`, `state_size_estimate`, `market_size_signal`, and the three beava-specific adoption signals (`existing_public_users_or_case_studies`, `community_discussions`, `examples_or_demos_in_repo`). These are flagged where relevant but omitted from the per-item summaries to keep the report readable.

---

## Executive summary

**Where beava obviously wins.** A handful of domains map almost 1:1 to beava's primitives (counters, velocities, distances, distributions, atomic per-event update-then-read), tolerate single-binary deployment, and have buyer pain that current Redis+Lua or Redis+Flink stacks badly handle:

- **Impossible-travel / geo-velocity (#19)** — beava already ships `geo_velocity`, `geo_distance`, `distance_from_home` as first-class primitives. No competitor's online feature store exposes geo primitives this way. **Fastest demo in the catalog (1 day).**
- **Edge / CDN per-customer rate-limit (#15c)** — single binary, sub-ms reads, push-on-event. The Cloudflare DO / Fastly ratecounter / Upstash slot.
- **Usage-based billing / metering (#14b)** — Stripe's $1B Metronome acquisition (Jan 2026) is the highest commercial signal in the catalog. beava's WAL + sub-ms reads beat Redis on durability; trails Lago/OpenMeter on event archive.
- **AI / LLM gateway features (#16)** — 2025–2026 demand wave (Portkey, LiteLLM, Helicone, Bifrost, Envoy AI Gateway). Per-(key × model) TPM, agent cost meters, jailbreak-velocity all map directly.
- **Real-time / dynamic pricing inputs (#12) and mobility dispatch (#12b)** — clean primitive fit; Uber Gairos publicly does "54 features per H3 hex per minute" which is exactly the @bv.table shape.

**Where beava could win with one engineering push.** Several use cases are blocked on the same short list of v0 gaps:

1. **TLS + auth** — universal blocker. Mentioned in nearly every item.
2. **Triggers / fire-on-threshold-cross** — the missing primitive that converts beava from a feature store into an event-driven decisioning engine. Critical for #14b (billing caps), #17 (lifecycle journeys), #23b (crypto withdrawal governance), #15b (WAF inline action), #25 (telecom IRSF blocking), #7b (gateway-health circuit-breaker).
3. **In-process sharding** — caps deployment at "regional single-binary." Tier-1 ad-tech / global card networks / Coinbase-class crypto exchanges exceed this. Mid-market is fine today.
4. **Multi-tenancy / namespace ACLs** — required for any vendor-embedded play (T&S vendors, AI gateways, edge providers).
5. **Point-in-time / features_snapshot_id** — required for any regulated decisioning (BNPL #7, AML #8, insurance #26): "what features fired this decision, exactly, for audit replay."

**Where beava is the wrong tool today.** Behavioral analytics dashboards (#5) and pure recsys (#18b) read-heavy fan-out shapes are fine, but the buyer for those already pays Mixpanel/Amplitude or Tecton/Hopsworks and beava has no offline-store / training-serving parity story. SIEM (#15) is technically a perfect fit but the SOC trust-boundary makes the TLS/auth gap a *harder* blocker than anywhere else.

**Where this research validates the README pitch.** Items 1a–5 (fraud, ad-tech, behavioral analytics) — the README-stated targets — all came back as strong technical fits. The expansion into adjacencies (items 6–27) shows that the same primitives credibly serve another 20+ domains; the constraint is not feature surface, it's the production-readiness gaps above.

**Highest-leverage shippable artifacts (from `examples_or_demos_in_repo` gaps).** Demo coverage today is `python/beava/demos/{adtech, ecommerce, fraud}`. Demos that would unlock specific use cases at near-zero engineering cost:

- `demos/impossible_travel/` — flagship for the distance primitive (#19)
- `demos/billing_meter/` — Stripe Meters look-alike (#14b)
- `demos/edge_rate_limit/` — sliding-window per-key counter (#15c)
- `demos/llm_gateway/` — per-key TPM + jailbreak-velocity (#16)
- `demos/aml/` — structuring / mule-velocity (#8)
- `demos/recsys_ranking/` — per-user last-N + per-item rolling CTR (#6, #18b)
- `demos/anti_cheat/` — per-player APM / win-streak (#11)

---

## Contents

Each entry: `id` — name — adversarial-pressure tag.

**README-stated targets (current positioning)**
1. [1a — Fraud, card-not-present checkout / transaction fraud](#1a-fraud-cnp) — High / active
2. [1b — Fraud, instant-payments / real-time rails (FedNow, UPI, SEPA Instant, RTP)](#1b-fraud-instant) — Maximum
3. [1c — Fraud, Authorized Push-Payment / social-engineering scams](#1c-fraud-app) — High and rising
4. [2 — Account abuse / bot defence](#2-account-abuse) — Maximum
5. [3 — Ad-tech frequency capping & pacing](#3-frequency-cap) — Low-to-moderate (passive)
6. [4 — Ad-tech click/install fraud & IVT](#4-ivt) — High / active
7. [5 — Behavioral analytics, live product analytics](#5-analytics) — Low / passive

**Adjacent, strong technical fit**
8. [6 — Real-time personalization / ranking & recsys](#6-ranking) — Passive load
9. [7 — Real-time credit / BNPL / KYC risk decisioning](#7-credit-bnpl) — High and rising
10. [7b — PSP / payment-orchestration real-time risk](#7b-psp) — High + passive mix
11. [8 — AML / transaction monitoring](#8-aml) — High, slower tempo
12. [9 — Trust & safety / abuse detection on UGC platforms](#9-ts-ugc) — Maximum
13. [9b — Trust & safety on live-streaming](#9b-livestream) — Maximum
14. [10 — Marketplace integrity (supply-side abuse)](#10-marketplace) — Maximum
15. [11 — In-game anti-cheat & player-behaviour features](#11-anti-cheat) — Maximum
16. [11b — Game economy / virtual-currency AML (RMT, gold-farming)](#11b-game-aml) — High, continuous
17. [12 — Real-time / dynamic pricing inputs](#12-pricing) — Mixed, mostly passive
18. [12b — Mobility / dispatch surge & ETA features](#12b-dispatch) — Low to moderate
19. [13 — IoT / device telemetry rollups + anomaly windows](#13-iot) — Mostly passive
20. [14 — Observability, per-tenant/per-route rolling rates](#14-observability) — Low / passive
21. [14b — Usage-based billing / metering / SaaS quotas](#14b-billing) — Moderate
22. [15 — Network security / SIEM enrichment](#15-siem) — Maximum
23. [15b — API abuse / bot defence / WAF feature backend](#15b-waf) — Maximum
24. [15c — Edge / CDN per-customer rate-limit & usage features](#15c-edge) — Mixed
25. [16 — LLM / AI-gateway feature backend](#16-llm) — High and rising
26. [17 — Conversion / lifecycle marketing triggers](#17-lifecycle) — Low / passive
27. [18a — Online feature store for tabular fraud/risk classifiers](#18a-fs-fraud) — High / active
28. [18b — Online feature store for ranking / recsys](#18b-fs-recsys) — Passive load

**New, surfaced by web-search supplement**
29. [19 — Impossible-travel / geo-velocity for identity & ATO](#19-impossible-travel) — High
30. [20 — Device intelligence / device-graph velocity](#20-device-intel) — Maximum
31. [21 — On-demand marketplace matching & pricing](#21-marketplace-matching) — Low to moderate
32. [22 — Live sports betting risk & odds management](#22-betting) — Maximum-adjacent
33. [23 — Crypto / web3 wallet risk & on-chain monitoring](#23-crypto-wallet) — Very high
34. [23b — Crypto-exchange withdrawal-velocity governance](#23b-crypto-withdrawal) — Very high, fast-tempo
35. [24 — Bandit / contextual-bandit & online RL serving features](#24-bandit) — Mostly passive
36. [25 — Telecom CDR fraud (wholesale / IRSF / bypass)](#25-telecom) — High / active
37. [26 — Real-time insurance underwriting & claims fraud at FNOL](#26-insurance) — High and rising
38. [27 — Supply chain / inventory real-time stock & velocity](#27-supply-chain) — Mostly passive

---

## README-stated targets

### <a id="1a-fraud-cnp"></a>1a — Fraud, card-not-present checkout / transaction fraud

**Adversarial pressure:** High / active. Fraudsters adapt to detection rules within hours.

**Domain shape.** Auth-time scoring under a ~100 ms end-to-end budget (Stripe Radar: ~1–2 ms ML inference + ~98 ms feature collection). Multi-grain entity keys (card_hash, account_id, device_id, ip, bin, (card_hash × merchant_id) pair, email_hash). Mixed exact (accounting-grade velocity rules) and approximate (HLL/CMS) aggregates. Both sliding and tumbling windows; session windows for ATO.

**Buyer.** Risk / fraud-ops team at PSPs, acquirers, issuers, marketplaces, large merchants. ML platform secondary.

**Market.** Global CNP losses ~$28.1B by 2026 / ~$49B by 2030; US CNP $10.16B in 2024 (74 % of US card-payment fraud). Fraud-detection software TAM $51–67B in 2025–2026.

**beava fit.** Sub-ms TCP `batch_get` fits the budget with margin (Stripe Radar / Tecton 0.8 ms / Aerospike Barclays sub-100ms p99). Single-binary fits regional PSP scale; not a global card network. Repo already ships `python/beava/demos/fraud/` (schema.json + events.jsonl) and `crates/beava-bench/configs/fraud-team.json`.

**Blocking gaps.** TLS, auth, in-process sharding, cross-region replication, triggers (fraud teams want push not poll), multi-tenancy, secondary indexes, native PCI-DSS posture documentation.

**Competitors.** Redis+Lua, Aerospike, Tecton (0.8 ms feature retrieval ref), Hopsworks, Feast-on-Redis, Chalk, Fennel, plus specialist vendors Sift, Forter, Riskified, Signifyd, FICO Falcon, Featurespace, Feedzai.

→ [`results/1a_fraud_card_not_present.json`](results/1a_fraud_card_not_present.json)

---

### <a id="1b-fraud-instant"></a>1b — Fraud, instant-payments / real-time rails (FedNow, UPI, SEPA Instant, RTP)

**Adversarial pressure:** Maximum. Adversaries iterate in hours, run mule-account farms.

**Domain shape.** End-to-end single-digit seconds (SEPA Instant 10 s hard window; UPI request/response reduced to 15 s in June 2025). Pre/intra-transaction decisioning, not minutes-later batch. Distinct from card fraud because the transaction is irrecoverable after settlement.

**Buyer.** Bank fraud + AML teams running rails participation (Tier-1 banks, EMIs, neobanks).

**Tailwinds.** FedNow $10M limit lift Nov 2025; RTP $10M Feb 2025; SEPA Instant mandatory Oct 2025; EU Verification of Payee mandatory Oct 2025 / Jul 2027; UK PSR APP-reimbursement live Oct 2024.

**beava fit.** ~684k single-core EPS is 2 orders of magnitude above any single-tenant rails workload. Feature shape (per-payer velocity, per-counterparty, first-time-payee) maps cleanly. Bottleneck is feature count per decision (20–200+) and state size, not raw EPS.

**Blocking gaps.** TLS, auth, replication, triggers, sharding. Universal blockers for regulated rails.

**Competitors.** Featurespace (Visa-acquired), NICE Actimize, Feedzai, Hawk.AI, ComplyAdvantage, Flagright, ACI. beava plausibly sits *underneath* these as a Redis-replacement substrate, not head-to-head.

→ [`results/1b_fraud_instant_payments.json`](results/1b_fraud_instant_payments.json)

---

### <a id="1c-fraud-app"></a>1c — Fraud, Authorized Push-Payment / social-engineering scams

**Adversarial pressure:** High and rising. Fraud rings rotate mule accounts and scripts; exploit each new rail within months.

**Domain shape.** The account-holder authorises the payment under coercion. Per-(payer × counterparty) velocity, first-time-payee + large-amount features, behavioral session features (typing pauses, copy-paste of recipient), peer-group comparisons.

**Buyer.** Tier-1 banks under PSR liability; challenger banks; EMIs. UK Finance reports £450.7m APP losses in 2024; £173m reimbursed in the first PSR year.

**Tailwinds.** UK PSR mandatory reimbursement (Oct 2024) splits costs 50/50 sender/receiver up to £85k. Australian SPF (Feb 2025); Singapore SRF (Dec 2024).

**beava fit.** Feature shapes map cleanly to @bv.table primitives. Latency budget is comfortable (<2–3s envelope; feature-fetch <50ms p99). Inbound mule features (sudden inbound from new payers, forward-out rate) are the receiving-PSP side that became commercially urgent under the 50/50 split.

**Blocking gaps.** TLS + auth, replication, features_snapshot_id for PSR/FOS replay, triggers for step-up flows, sharding for the (payer × counterparty) long-tail (~1.5B pairs at tier-1 scale).

**Competitors.** Featurespace ARIC Scam Detect (NatWest, ClearBank), Mastercard Consumer Fraud Risk (nine-UK-bank consortium), BioCatch Scams360, ThreatMark. beava wedge: developer-facing feature plane below the scoring engine, esp. at challenger banks / EMIs.

→ [`results/1c_fraud_app_scams.json`](results/1c_fraud_app_scams.json)

---

### <a id="2-account-abuse"></a>2 — Account abuse / bot defence

**Adversarial pressure:** Maximum. Attackers retool weekly (AI-driven stuffing, residential proxies, JA4-spoofing, headless polymorphism).

**Domain shape.** Per-IP, per-fingerprint, per-route, per-(IP × UA) velocity. Credential-stuffing waves (many usernames against one host), signup floods, scraping. Sumsub finding: 76 % of fraud now happens *after* identity verification.

**Buyer.** Platform/infrastructure team (Castle, Fingerprint, Cloudflare Bot Management), distinct from app fraud team.

**Market.** Bot management $1–1.3B in 2025, 18–24% CAGR. Akamai: ~193B credential-stuffing attempts/year industry-wide.

**beava fit.** Strong primitive coverage; single-binary edge deployment is uniquely viable. Auth0 / Akamai / Verizon DBIR all describe the exact shape beava implements.

**Blocking gaps.** TLS, auth, sharding, triggers, multi-tenancy — disqualifying for IDP / bot-management / WAF buyer personas. Add Rust/Go SDKs (current Python-only doesn't fit edge runtimes).

**Competitors.** Castle, Fingerprint, Cloudflare Bot Management, DataDome, Kasada, Arkose, HUMAN, Auth0. PSD3/PSR mid-2026 plus AI-driven credential stuffing are the 2026 tailwinds.

→ [`results/2_account_abuse_bot_defence.json`](results/2_account_abuse_bot_defence.json)

---

### <a id="3-frequency-cap"></a>3 — Ad-tech frequency capping & pacing

**Adversarial pressure:** Low-to-moderate / passive load. Counter-parties are honest exchanges and bidders.

**Domain shape.** Per-(user × campaign × creative) impression caps; per-campaign pacing (smooth vs ASAP). Bid-decision budget single-digit ms at SSP/DSP/exchange. Strict no-double-count for cap honour; cookieless Privacy Sandbox Protected Audience / TURTLEDOVE compatibility.

**Buyer.** DSPs (TheTradeDesk, DV360), SSPs/exchanges (Magnite, OpenX, PubMatic), CTV ad-servers.

**beava fit.** Primitive fit is excellent (counters, velocities, distributions, recency over user × campaign cross-product). Repo already ships `python/beava/demos/adtech/` with Impression/Click/Conversion — a frequency-cap @bv.table is an afternoon away.

**Blocking gaps.** Cross-region active-active counter consistency (Aerospike XDR is the incumbent feature for this), in-process sharding (tier-1 DSP scale is 10–100× one box), idempotency-by-bid_id, triggers for pacing thresholds.

**Competitors.** Aerospike dominates (Trade Desk, Criteo ~290M QPS aggregate, fuboTV, MGID); Redis / ElastiCache for mid-market (AWS Architecture Blog: "Serving Billions of Ads in 100 ms"). Privacy Sandbox Protected Audience is the parallel on-device path beava can't replace.

→ [`results/3_adtech_frequency_capping.json`](results/3_adtech_frequency_capping.json)

---

### <a id="4-ivt"></a>4 — Ad-tech click/install fraud, invalid-traffic (IVT)

**Adversarial pressure:** High / active.

**Domain shape.** Pre-bid IVT ~10 ms (HUMAN spec); attribution-time MMP detection 5–15 s. Detection turns on *distribution shape* (CTIT histogram, click-cadence, viewable-ms) and *ratio aggregates* (click-to-install). Per-click, per-device, per-publisher rolling rates.

**Buyer.** MMPs (AppsFlyer, Adjust, Branch), SSPs/exchanges, advertisers.

**Market.** 2025 global ad-fraud losses $32.6B (Spider Labs) to $100B+ (industry consensus); projected $172B by 2028. Global IVT rate 20.64% (Fraudlogix).

**Standards.** MRC IVT Detection 2024 update; TAG Certified Against Fraud v7.3 (326 seals in 2025); IAB sellers.json / SupplyChain Object.

**Blocking gaps.** TLS/auth (non-starter for MMP/SSP security review), sharding for global exchange scale (1T+ data points/month at TrafficGuard, 3M QPS at major DSPs), triggers for pause-payout actions, multi-tenancy, surfaced HLL/CMS/t-digest as first-class primitives.

→ [`results/4_adtech_ivt_click_fraud.json`](results/4_adtech_ivt_click_fraud.json)

---

### <a id="5-analytics"></a>5 — Behavioral analytics, live product analytics

**Adversarial pressure:** Low / passive. No adversary; drift is from product changes.

**Domain shape.** Per-user/per-session live counters (sessions_today, events_in_window, time-to-Nth-event, last-active, page-velocity, funnel-step counters). Read-heavy (dashboards, in-app personalization, CDP, lifecycle marketing).

**Buyer.** Product / growth / data team — *not* fraud team.

**beava fit.** The freshness gap is the real wedge. Mixpanel ingests under 1 min (mobile +1–2 min buffer); Amplitude streaming export ~60 s p95; Adobe RTCDP streaming-segmentation up to 5 min; Snowflake Dynamic Tables 1-min minimum lag; warehouse+dbt typically 5–60 min. beava is sub-second.

**Market.** USD ~10.6–14.8B in 2025; 12–23% CAGR to $30–41B by 2030–2034. PostHog/Mixpanel/Amplitude per-event pricing is the textbook seam pushing larger buyers toward a hybrid self-hosted hot tier.

**Likely deployment.** *Embeds under* (not replaces) Mixpanel/Amplitude/PostHog/Segment/RudderStack/Hightouch.

**Blocking gaps.** TLS+auth (blocks any PII deployment), TTL/eviction on explosive anonymous_id cardinality, HLL/CMS as first-class primitives, GDPR-style tombstone-by-user_id, triggers (for lifecycle-marketing slice), identity-stitch on anonymous_id→user_id.

→ [`results/5_behavioral_analytics.json`](results/5_behavioral_analytics.json)

---

## Adjacent, strong technical fit

### <a id="6-ranking"></a>6 — Real-time personalization / ranking & recsys

**Adversarial pressure:** Passive load (modulo gaming via separate T&S logic).

**Domain shape.** Read-heavy fan-out: one user request → 100–500 batched feature reads (1 user × 30 features + N candidates × 30 features). Qualitatively different from fraud's write-heavy shape.

**Public references.** Chalk + Whatnot at 300M features/sec in livestream marketplace; Apartment List sub-5 ms ranking; DoorDash >1M predictions/sec, >20M reads/sec; Instacart Griffin.

**beava fit.** Sub-ms TCP `batch_get` fits the 5–30 ms feature-fetch sub-budget inside 100–200 ms ranking end-to-end. HTTP/TCP push maps to webhook + mobile-SDK paths.

**Blocking gaps.** No published `batch_get` benchmark at 100–500 candidate fan-out; no vector / ANN index (beava covers ranking-stage features, *not* retrieval-stage candidate generation); no point-in-time-correctness story for training/serving consistency; sharding caps deployment at mid-sized marketplaces.

→ [`results/6_personalization_ranking.json`](results/6_personalization_ranking.json)

---

### <a id="7-credit-bnpl"></a>7 — Real-time credit / BNPL / KYC risk decisioning

**Adversarial pressure:** High and rising (AI-driven synthetic-identity).

**Domain shape.** Per-applicant rolling features at decision time: thin-file behavioral signals (cashflow velocity, account age, device tenure), KYC velocity (apps-per-device, apps-per-email-domain), affordability windows. Decision budget hundreds-of-ms to a few seconds.

**Buyer.** BNPL / consumer-lender (Affirm, Klarna, Afterpay/Block, Sezzle, Zip, Mercury, Brex).

**Throughput context.** Klarna ~2.9M txn/day (~34 EPS avg); PayPal Pay-in-4 "thousands of decisions/sec at peak." beava has multi-order-of-magnitude headroom for all named buyers except global-scale Klarna peak.

**Regulatory tailwinds.** FCA DPC regime 15 Jul 2026; ASIC RG 281 (Jun 2025); EU PSD3/PSR1 mid-2026; US CFPB BNPL interpretive rule withdrawn May 2025.

**Domain-unique gap.** **Point-in-time coherent multi-grain feature snapshot** for fair-lending audit. RisingWave explicitly calls this out; beava has no native `features_snapshot_id` primitive.

**Closest competitor.** Chalk with explicit MoneyLion + Mission Lane + iwoca underwriting case studies.

→ [`results/7_credit_bnpl_kyc.json`](results/7_credit_bnpl_kyc.json)

---

### <a id="7b-psp"></a>7b — PSP / payment-orchestration real-time risk

**Adversarial pressure:** High + passive load mix. BIN-attack/card-testing surface is fraud-like; routing telemetry is passive.

**Domain shape.** Processor-side features at routing/auth time: merchant decline-rate velocities, BIN-route success-rate velocities, retry-and-cascade detection, real-time approval-rate steering, gateway-health features. Routing decisions need 20–50 ms.

**Buyer.** Adyen, Stripe, Checkout.com, Worldpay, payment orchestrators (Primer, Spreedly, Gr4vy), large in-house orchestrators (Uber, Airbnb, Booking).

**Public refs.** Stripe Adaptive Acceptance (10% false-decline recovery); Adyen RevenueAccelerate (26% cost savings, 0.22% auth uplift); JUSPAY Hyperswitch (200M daily txns, $670B TPV, Rust, open-source orchestrator — 25 ms whole-app latency).

**Domain-unique gap.** **Triggers** — the gateway-health circuit-breaker pattern is the #1 blocker. "When (acquirer, decline_code) decline-rate exceeds X in 60s, emit action" with sub-200ms switchover target. beava's poll-only model cannot hit this today.

**Validated by.** Chalk has explicitly productised "Real-Time Payment Risk Decisioning" as a named use case.

→ [`results/7b_psp_payment_orchestration.json`](results/7b_psp_payment_orchestration.json)

---

### <a id="8-aml"></a>8 — AML / transaction monitoring

**Adversarial pressure:** High, slower-tempo. Adversaries iterate over weeks-to-months, not minutes.

**Distinct from fraud.** AML is case-managed (SARs/CTRs filed to regulator), not auto-blocked. Latency tolerance seconds-to-minutes (historically overnight). State lifecycle materially longer: 90–365 day baselines, 5–7 year regulator retention.

**Throughput.** Heterogeneous: tens of EPS for a community bank to tens of thousands for top-tier global bank or crypto exchange. beava has huge headroom; long state lifecycle stresses in-memory-only.

**Market.** AML software ~$4.1B (2025) → $9.4B (2030). Transaction-monitoring market ~$16–20B (2025) → $47.6B (2033). US 2025 AML enforcement penalties >$940M. EU AMLA operational 1 Jul 2025; AMLR/AMLD6 apply 10 Jul 2027 — drives a wave of re-tooling.

**Critical regulatory gaps beyond fraud's list.** Point-in-time / as-of reads (to reproduce features at decision time for SAR audits), per-read audit log, deterministic WAL replay, data-residency controls, GDPR right-to-erasure without corrupting aggregates.

**OSS competitors specifically in this lane.** Jube (AGPLv3, in-memory features for sub-ms decisioning), Marble (real-time decision engine for fraud + AML) — direct positional overlap.

→ [`results/8_aml_transaction_monitoring.json`](results/8_aml_transaction_monitoring.json)

---

### <a id="9-ts-ugc"></a>9 — Trust & safety / abuse detection on UGC platforms

**Adversarial pressure:** Maximum. Attackers retool weekly (LLM-spam, sockpuppet networks, coordinated brigading).

**Domain shape.** Per-author post velocity, per-target unique-reporter counts (canonical mass-reporting signal), per-(reporter, target) for retaliatory/brigading detection, per-room raid signals, per-replier target-diversity (reply-flood detection).

**Regulatory cliff edges (urgency drivers).** EU DSA Implementing Regulation 2025/1714 (data collection started 1 Jul 2025, first harmonised reports early 2026); UK Online Safety Act illegal-harms duties (17 Mar 2025) and children's-safety duties (25 Jul 2025); Ofcom 10%-of-revenue fines, first £1M+ fine in 2025; US KOSA; EU CSAM derogation lapse Apr 2026.

**Market.** USD ~10–15B in 2025, 13–15% CAGR to $26–42B by 2030–2035.

**Incumbents.** Redis+Lua is overwhelming default; Aerospike at high cardinality. Vendors: ActiveFence/Alice (post Spectrum Labs), Cinder, TrustLab, Resolver, Hive AI, Sift Content Integrity, Sumsub, Two Hat/Microsoft. In-house at Reddit, Discord, Roblox (Sentinel Aug 2025), Meta, Bluesky.

**Critical engineering gaps.** TLS/auth, triggers/threshold callbacks (most-cited workflow gap), HLL/CMS top-K primitives, in-process sharding, multi-tenancy, native (pair) composite-key aggregations, GDPR/DSA PII story.

→ [`results/9_trust_safety_ugc.json`](results/9_trust_safety_ugc.json)

---

### <a id="9b-livestream"></a>9b — Trust & safety on live-streaming

**Adversarial pressure:** Maximum. Hate raids retool weekly (residential-proxy bot networks, LLM-spam, deepfake voice).

**Distinct from #9.** Seconds budget (Roblox 15s end-to-end voice-action SLO; academic hate-raid measurements show thousands of messages in <16 s). Entity grain is ephemeral (room_id / channel_id / stage_id / lobby_id / stream_id). Event shapes: chat_message, voice_flag, viewer_join, raid_in, stage_promote, clip_create.

**Buyer.** Twitch, YouTube Live, Kick, TikTok Live, Bigo, Roblox, Discord Stage, in-game voice (Riot, Activision, Epic). Plus livestream commerce (Whatnot, Amazon Live).

**Competitors.** Modulate ToxMod, Roblox Sentinel (open-sourced Aug 2025), ActiveFence+Modulate joint offering, Agora SDK with embedded ActiveFence, GetStream Chat.

**Domain-unique gaps.** Triggers gap is bigger here than #9 — action surface (slow-mode, followers-only, kick, demote-from-stage) is tightly threshold-coupled at seconds-cadence. GDPR Art. 9 voice-biometric handling is a load-bearing engineering gap.

→ [`results/9b_livestream_moderation.json`](results/9b_livestream_moderation.json)

---

### <a id="10-marketplace"></a>10 — Marketplace integrity (supply-side abuse)

**Adversarial pressure:** Maximum. Counterfeiters, drop-shippers, fee-arbitrage rings, refund-abuse rings.

**Domain shape.** Per-seller listing velocity, per-seller refund-rate over rolling windows, per-(seller, buyer) interaction count, image-reuse / duplicate-detection metadata counters, return-abuse counters, drop-shipping velocity signatures. **Composite-key grains** ((seller, payout_account), (image_hash → n_unique seller_id)) are the load-bearing primitives.

**Rare clean alignment.** Sliding windows align with regulator-enforced metrics: Amazon's 5–8% rolling-return-rate threshold, Walmart's Seller Performance Standards, eBay defect-rate windows. Most generic feature stores don't align this neatly with platform enforcement.

**Urgency drivers.** EU DSA Art. 30 KYBC, US INFORM Consumers Act, EU GPSR (Dec 2024), Amazon's 2024–2026 returns-policy tightening, CNBC Sep 2025 Walmart Marketplace exposé, 180% YoY rise in coordinated attacks (Sumsub 2025–2026).

**Gaps.** Same as #1a / #9 priority order: TLS+auth, triggers, composite-key first-class, in-process sharding, multi-tenancy, secondary indexes, PII-erasure cascade, image-hash sidecar pattern.

→ [`results/10_marketplace_integrity.json`](results/10_marketplace_integrity.json)

---

### <a id="11-anti-cheat"></a>11 — In-game anti-cheat & player-behaviour features

**Adversarial pressure:** Maximum. Cheats retool weekly (DMA hardware cheats, CV aimbots, AI-driven scripting).

**Domain shape.** Per-player headshot rate, APM, win-streak velocity, per-lobby kick-rate, per-IP/per-account aimbot signal aggregation, smurf detection from (account, MMR) tenure features. Server-side telemetry complements client-side anti-cheat (Vanguard, EAC, BattlEye, RICOCHET).

**Throughput signals.** Fortnite ~125M events/min ≈ 2M EPS; Roblox 120M data points/sec; Valorant ~60k bans/week.

**Public refs.** Riot Data + AI Summit 2025 talk; Activision RICOCHET ML aimbot detection; Roblox Sentinel preemptive risk detection (open-sourced Aug 2025); Riot Vanguard kernel-level architecture; Anybrain, GGWP, Modulate, Getgud vendor lines.

**Regulatory pressure.** EU AI Act (high-risk classification possible for ban decisions); EU DSA reporting; UK OSA; GDPR/CCPA on telemetry retention; KOSA. Helldivers 2 / 2K Borderlands EULA cases set anti-cheat legal precedents.

→ [`results/11_anti_cheat.json`](results/11_anti_cheat.json)

---

### <a id="11b-game-aml"></a>11b — Game economy / virtual-currency AML (RMT, gold-farming)

**Adversarial pressure:** High and continuous. Gold-farming orgs are professional, multi-continent operations.

**Distinct from #11.** Economy integrity vs match-result fairness. Per-account currency-in/out velocity, per-account trade-network features (degree, density, sudden new counterparty), gold-farming signatures (24/7 grind detection, repetitive route distance — beava's distance primitive maps directly), RMT detection, item dumping/laundering patterns.

**Scale signals.** Roblox markets "billions of data points in milliseconds" for Risk & Fraud; Jagex banned 4.29M OSRS accounts in 2025; CCP banned ~70k EVE accounts in 2021; Roblox 2025 revenue ~$4.9B, bookings ~$6.8B; CS2 skin economy $4.3–5.2B in 2025; NY AG sued Valve in Feb 2026 over loot boxes / skin gambling.

**Domain-unique blockers.** Triggers (event-driven "fire BAN_CANDIDATE when features cross") is the highest-leverage; TLS+auth second. Roblox DevEx / blockchain games cross into FinCEN MSB / FATF VASP territory — adds AML retention requirements.

→ [`results/11b_game_economy_aml.json`](results/11b_game_economy_aml.json)

---

### <a id="12-pricing"></a>12 — Real-time / dynamic pricing inputs

**Adversarial pressure:** Mixed, generally passive load with adversarial sub-paths (competitor scraping / counter-scraping).

**Domain shape.** 8 distinct buyer verticals (retail/e-comm, ride-hailing, food delivery, airline RM, hotel RM, ticketing, marketplace seller repricing, energy DR). All share the same feature-store-as-backbone shape: per-SKU / per-(SKU, zone) / per-(origin, dest, date) / per-H3-cell counters, velocities, sell-through rates, look-to-book ratios, supply/demand ratios, competitor-price anchors.

**Benchmarks.** Tecton sub-10 ms p99 at 100k+ QPS; Hopsworks 7.5 ms p99 at 250k+ ops/sec; DoorDash Iguazu "hundreds of billions of events/day." beava 684k EPS / sub-ms sits favourably.

**Regulatory.** EU Omnibus Directive Art. 6a (30-day reference price disclosure, since May 2022); proposed Digital Fairness Act (Q4 2026); US Senate 2026 Ticketmaster Pricemaster report; state-level surge-pricing investigations.

**Gaps.** TLS+auth, triggers, multi-tenancy (for hotel-RM SaaS vendors), composite-key ergonomics, time-travel / point-in-time backfill for elasticity model training, secondary indexes for RM-analyst views.

→ [`results/12_dynamic_pricing.json`](results/12_dynamic_pricing.json)

---

### <a id="12b-dispatch"></a>12b — Mobility / dispatch surge & ETA features

**Adversarial pressure:** Low to moderate (some incentive gaming).

**Domain shape.** Per-H3/geohash cell rolling demand counters, per-driver acceptance / cancellation rate, per-driver idle-time, surge-multiplier inputs, per-restaurant prep-time velocity, per-route ETA-error feedback. Decision budget ~100 ms.

**Public refs.** Uber Gairos runs "54 features per H3 hex per minute via 9 rings × 6 window sizes" — near-perfect mapping to @bv.table semantics. Plus DoorDash Riviera / DeepRed, Lyft surge with Flink + H3, GoJek Surbo.

**Strong fit reason.** Mobility events are geo-local — single-region single-binary deployment is *better* fit here than for ad-tech.

**Gaps.** TLS/auth, no H3/S2/geohash *ring* primitive on the server side (beava has `geo_velocity` / `geo_distance` but not "9-ring × 6-window" out-of-the-box), in-process sharding for tier-1 single-region scale, no mobility/dispatch demo in `python/beava/demos/`.

→ [`results/12b_mobility_dispatch.json`](results/12b_mobility_dispatch.json)

---

### <a id="13-iot"></a>13 — IoT / device telemetry rollups + anomaly windows

**Adversarial pressure:** Mostly passive, localized adversarial pockets.

**Domain shape.** Per-device rolling counters of error events / sensor extremes / connectivity drops; per-fleet z-score distributions; predictive-maintenance feature precursors (delta-since-last-anomaly). Heterogeneity is huge: IIoT vs connected-vehicle vs smart-meter span 3 orders of magnitude in EPS.

**Market.** Predictive maintenance $9.2–14.3B (2025) → $40–98B (2030–2034) at 24–28% CAGR. IIoT platforms $11.1B (2025) → $30.3B (2033).

**Public refs.** Tesla Fleet Telemetry 500 ms cadence; Rivian + VW (RV Tech) 5,500 signals per car every 5 s, 88% volume reduction via Kafka+Flink; Southern Company 4.6M smart meters (Databricks); Aerospike PlayStation p95 ~2 ms lookup.

**Positioning.** "Hot feature tier" between broker (MQTT/Kafka/AWS IoT/Azure IoT Hub) and TSDB (Influx/Timescale/ClickHouse/ADX) — *aggregator above* the TSDB, not replacement.

**Gaps.** TLS/auth (OT/IT boundary), sharding (national fleets exceed single node), MQTT/Sparkplug adapter, triggers (anomaly windows exist to fire actions).

→ [`results/13_iot_telemetry.json`](results/13_iot_telemetry.json)

---

### <a id="14-observability"></a>14 — Observability, per-tenant / per-route rolling rates

**Adversarial pressure:** Low / passive at SRE core, moderate at the inline-decisioning edge.

**Domain shape.** Per-tenant/per-route/per-method rolling rates (RPS, error rate, p95 latency window), per-tenant resource-use velocity, anomaly windows. The (tenant × route × method × status) keyspace becomes per-entity rows — sidesteps Prometheus cardinality explosion.

**beava angle.** Sub-ms TCP + msgpack reads make it viable as an inline rate-limit / admission-control gate at the API gateway — Prometheus query latency (100ms–10s) disqualifies generic TSDBs from this regime.

**Gaps.** No TLS/auth (#1 production blocker), no OTel-collector exporter, no Prometheus remote-write receiver, no Grafana data-source plugin, no in-process sharding, no ad-hoc PromQL/SQL query surface, no DDSketch / HDR-histogram / t-digest as first-class.

**Highest-leverage shippable.** Prometheus remote-write receiver — would let beava sit transparently behind existing scrape configs.

**Buyers.** Platform/SRE teams hitting Datadog custom-metric cardinality bills; multi-tenant SaaS noisy-neighbor / admission-control; AI/LLM platform teams needing per-tenant token-velocity; edge/API-gateway teams (Envoy, Kong, Cloudflare).

→ [`results/14_observability.json`](results/14_observability.json)

---

### <a id="14b-billing"></a>14b — Usage-based billing / metering / SaaS quotas

**Adversarial pressure:** Moderate. API-key leak/share, scraping below quotas, dispute-of-record incentives.

**Domain shape.** Two latency regimes: inline quota/credit gates <5–10 ms p99 (beava sub-ms wins); invoicing tolerates seconds-to-minutes (Lago/OpenMeter/Orb fine).

**Top commercial signal in the catalog.** Stripe's $1B Metronome acquisition Jan 2026; Patrick Collison: "metered pricing is the native business model for the AI era."

**Single biggest gap for beava.** **Durability.** SOX / 7-year retention regulator-grade domain; beava v0 in-memory + WAL+snapshot is hot-state-only. Durable raw-event log surviving independently of snapshot, with cold archive to S3/Parquet (object-lock), is required for beava to be *source-of-truth* metering. As an *upstream pre-aggregator* in front of Lago/Stripe/Metronome, the gap is much smaller.

**Competitors.** Commercial: Stripe Billing Meter (10k req/s livemode), Metronome (now Stripe), Orb (250k+ EPS), Chargebee, Zuora. OSS: Lago (1M EPS on ClickHouse Cloud, Apache-2.0), OpenMeter (YC-backed, Apache-2.0).

→ [`results/14b_usage_billing_metering.json`](results/14b_usage_billing_metering.json)

---

### <a id="15-siem"></a>15 — Network security / SIEM enrichment

**Adversarial pressure:** Maximum. Low-and-slow tuning, randomized C2 beaconing.

**Strongest technical fit, hardest commercial blocker.** Port-scan literally is `n_unique(dest_port) per src_ip in 60s`; brute-force is `count(failed_login) per src_ip per account in 5m`; C2 beaconing is `inter-arrival-time stddev per (src×dst×port)`; impossible-travel is `distance(last_geo, current_geo) / time_delta`. But v0 lack of TLS, auth, audit logging, FIPS-validated crypto, multi-tenancy is **an absolute disqualifier** — SOC traffic crosses trust boundaries by definition; PCI-DSS / HIPAA / FedRAMP / NIS2 all explicitly require it.

**Realistic seam.** "Hot pre-aggregation tier between event ingest (Cribl Stream / Bindplane / OTel-collector / Zeek / NetFlow) and SIEM cold tier (Splunk ES, Sentinel, Chronicle, Elastic Security, Falcon LogScale, Sumo, Datadog Cloud SIEM, Panther)" — *not* SIEM replacement.

**Market.** SIEM ~$12B → $20B by 2031, 11.5% CAGR. Falcon LogScale $585M ARR. Splunk ES list $250–400/GB/day; Sentinel $5.22/GB pay-as-you-go — per-GB ingest is buyer's #1 pain, exactly what pre-aggregation relieves.

**Buyer.** Detection engineering / SecOps platform — *not* the SOC analyst.

→ [`results/15_siem_network_security.json`](results/15_siem_network_security.json)

---

### <a id="15b-waf"></a>15b — API abuse / bot defence / WAF feature backend

**Adversarial pressure:** Maximum.

**Distinct from #2 (account-abuse) and #15 (SIEM).** Action is request-blocking / throttling / challenge, not SOC triage. Entity grain is composite around (route_template, api_key, status_class, ASN, JA4) — distinct from #2's identity-centric grain. Sub-second windows (1s, 10s) matter at this layer because every request is an inline decision.

**Closest commercial analog to beava's primitive shape.** Fingerprint's Velocity Signals (per-VisitorID/LinkedID/IP at 5m/1h/24h).

**Buyer.** Platform/edge-eng/SecOps at WAAP, CDN, API-gateway and bot-management vendors (Cloudflare, Fastly, Akamai, AWS, F5, Imperva, Datadome, HUMAN, Kasada, Arkose, Castle, Fingerprint, Kong, Apigee, Tyk, Wallarm, Salt, Noname/Akamai, Stytch).

**Top gaps.** TLS+auth, triggers (fire when threshold crosses), in-process sharding, HLL/CMS first-class, Rust/Go/Lua/Wasm client SDKs, cross-PoP feature merge.

→ [`results/15b_api_abuse_waf.json`](results/15b_api_abuse_waf.json)

---

### <a id="15c-edge"></a>15c — Edge / CDN per-customer rate-limit & usage features

**Adversarial pressure:** Mixed. Rate-limit / billing-meter are passive-load; DDoS / scraping / bot dimension is adversarial.

**Strong fit signals.** Sub-ms TCP + msgpack matches inline edge budget (5–20 ms total; rate-limit lookup ~1 ms). Single-binary per region/PoP matches how Cloudflare and Fastly actually deploy. 50+ aggregation primitives cover BOTH rate-limit (count/velocity/sliding) AND usage-meter (sum/tumbling) — incumbents need TWO systems (Redis-cell + OpenMeter/Lago).

**Critical gaps preventing v0 adoption.** No TLS/auth on any wire surface (disqualifying for multi-tenant edge); no multi-tenancy primitives; no cross-region async counter sync (Cloudflare per-PoP-with-convergence pattern); no billing-grade idempotency-by-event-id primitive; no triggers; major edge runtimes (CF Workers, Vercel Edge) are HTTP-only outbound — framed-TCP advantage only applies to self-hosted gateway sidecars.

**Competition.** Upstash @upstash/ratelimit (serverless de-facto), Cloudflare Durable Objects + native Rate Limiting API (GA Sept 2025), Fastly Edge Rate Limiting / ratecounter, redis-cell, Gubernator, Envoy Rate Limit, OpenMeter, Lago, Stripe Meters, plus per-vendor gateway plugins (Kong, APISIX, Tyk, Apigee).

→ [`results/15c_edge_cdn_rate_limit.json`](results/15c_edge_cdn_rate_limit.json)

---

### <a id="16-llm"></a>16 — LLM / AI-gateway feature backend

**Adversarial pressure:** High and rising (FlipAttack, PAIR, character-injection, encoded-prompt families).

**Domain shape.** Per-(api_key × model) TPM/RPM, per-tenant $-spend meters, per-agent step counters, per-tool-call frequency, jailbreak-pattern velocity (similarity to known jailbreak prompts in rolling window).

**Buyer.** AI-gateway vendors (Portkey, LiteLLM, Helicone, Bifrost, Truefoundry, agentgateway, Envoy AI Gateway, Kong AI, Cloudflare AI) + MCP gateways + agent-observability platforms (LangSmith, AgentOps, Galileo).

**Time-sensitive demand wave (2026).** Zuplo, Truefoundry, Portkey, agentgateway are all racing to ship token-aware rate limiting, per-tenant cost meters, agent-runaway kill-switches and jailbreak-velocity detection.

**Excellent shape match.** beava's @bv.event + @bv.table maps almost 1:1 to per-(api_key × model) TPM, per-tenant $-spend, per-agent tool-call-velocity feature shapes the entire market is hand-rolling on Redis + Lua today.

**Blockers.** No TLS/auth, no multi-tenancy primitives, no triggers, no Rust/Go/TS SDKs, no cross-node feature merge — all explicitly documented as features competing AI gateways ship on day one.

**Regulatory.** EU AI Act Art. 12 record-keeping, Art. 26 obligations of deployers — drives an audit-log-on-feature-read requirement.

→ [`results/16_llm_ai_gateway.json`](results/16_llm_ai_gateway.json)

---

### <a id="17-lifecycle"></a>17 — Conversion / lifecycle marketing triggers (event-driven journeys)

**Adversarial pressure:** Low / passive.

**Strong domain fit, one canonical gap.** README's positioning ("Replaces Postgres triggers + Redis counters + the cron job that heals drift") is *literally this use case*. Sub-second freshness directly maps to recovered revenue on cart-abandonment (~20% recovery at <1h vs ~12% at >24h).

**The blocking gap is the trigger primitive itself.** "Fire on threshold cross" stops being optional. Today the buyer must poll, which defeats the freshness pitch.

**Competitive landscape.** Customer engagement market ~$24B in 2025 → $57–86B by 2034. Klaviyo $1.2B FY2025 revenue; Braze ~$700M run-rate; Iterable $2B valuation. CDP-adjacent layer (Segment, RudderStack, mParticle, Hightouch, Census) is multi-billion. OSS self-hosted (Dittofeed, Laudspeaker, LimeJourney, Mautic) are natural embed targets.

**Positioning.** beava sits *underneath, not in place of*. Trigger feature backend behind Braze action-based delivery / Iterable Journeys / Customer.io / Klaviyo / OSS clones.

→ [`results/17_lifecycle_marketing_triggers.json`](results/17_lifecycle_marketing_triggers.json)

---

### <a id="18a-fs-fraud"></a>18a — Online feature store for tabular fraud/risk classifiers

**Adversarial pressure:** High / active (inherits #1a).

**Category role (not a use case).** beava as the online-FS half for tabular XGBoost / LightGBM / TabNet / TabTransformer fraud and risk classifiers. 20–100+ features per decision in <100ms; consistent point-in-time between training (offline) and serving (online).

**Performance comparables.** beava sub-ms TCP `batch_get` beats Vertex AI FS (~30 ms reported), matches Tecton (0.8 ms). Inside the 50–100 ms scoring budget JPMorgan ($2.4T/day, 9 ms p99) operates at.

**Market.** MLOps $4.39B → $89.91B (2026–2034, 45.8% CAGR); Fraud Detection $52.06B → $146.25B (2026–2033, 15.9% CAGR); BFSI 25.9% of MLOps spend; Tecton acquired by Databricks in 2025 — signals consolidation.

**Critical gaps vs incumbents.** No TLS/auth, no in-process sharding, no multi-region replication, no offline/online parity, no warehouse materialisation, no multi-tenancy, no tabular-ML-friendly SDK (`get_online_features(entity_rows=DataFrame)`), no feature monitoring/drift, no on-demand transformations, no managed offering.

→ [`results/18a_online_fs_fraud_classifiers.json`](results/18a_online_fs_fraud_classifiers.json)

---

### <a id="18b-fs-recsys"></a>18b — Online feature store for ranking / recsys

**Adversarial pressure:** Passive load.

**Distinct from 18a along three axes.**
- Workload: one ranking RPC fans out 50–500 candidates × 30 features ≈ 1,000–25,000 feature reads per call (qualitatively different from 18a's one-decision-one-batch).
- Freshness SLA: seconds (not sub-second) — explicit distinction from 18a. Pinterest reports ~10 s p99 signal-to-serving lag, +11% engagement.
- Throughput precedents: DoorDash >20M reads/sec on Redis; Whatnot on Chalk 300M+ features/sec; Hopsworks/RonDB 100M key lookups/sec; Swiggy 50M QPS on ElastiCache.

**beava's published 684k EPS is the write-side number.** The corresponding recsys-shape batched-read benchmark is the largest unproven datapoint.

**Biggest functional gap for procurement.** Point-in-time-correct offline materialization. Headline "consistency" feature Tecton/Hopsworks/Chalk/Fennel/Featureform sell.

→ [`results/18b_online_fs_recsys.json`](results/18b_online_fs_recsys.json)

---

## New — surfaced by web-search supplement

### <a id="19-impossible-travel"></a>19 — Impossible-travel / geo-velocity for identity & ATO

**Adversarial pressure:** High. Credential-stuffing toolkits, residential-proxy pools, session-token theft. But geolocation is *more stable* than fingerprint/UA.

**Perfect technical fit.** beava already ships `geo_velocity`, `geo_distance`, `geo_spread`, `distance_from_home` as first-class operators in `python/beava/_agg.py` (Rust impl in `crates/beava-core/src/agg_geo.rs`). **No other general-purpose feature backend exposes these as built-ins.** Cleanest "wow demo" beava can ship.

**Market.** ATO losses ~$17B in 2025 (up from $13B in 2023); credential-stuffing +148% YoY; 29% of U.S. adults hit by ATO in 2024. PSD3/PSR (in force Feb–Apr 2026) explicitly mandates real-time behavioural monitoring at login + sensitive actions.

**Latency / SLA.** Read p99 <10 ms inline sign-in target; <50 ms before login UX visibly degrades. beava sub-ms TCP fits with headroom; dominant external latency is IP-geo lookup (sub-ms with local MaxMind GeoLite2 DB).

**Competition.** Built-in implementations in Okta Velocity Behaviour, Microsoft Entra ID Protection, WorkOS Radar, Stytch, Castle, Fingerprint, Sift, SEON. beava sits *underneath* these as substrate, not replacement.

**Gaps.** TLS/auth, triggers (buyers want push-callbacks when km/h crosses threshold), cross-region replication, IP→geo enrichment (producer responsibility).

**Highest-leverage missing artefact.** A `python/beava/demos/impossible_travel/` example with synthetic login traces.

→ [`results/19_impossible_travel.json`](results/19_impossible_travel.json)

---

### <a id="20-device-intel"></a>20 — Device intelligence / device-graph velocity

**Adversarial pressure:** Maximum. Anti-detect browsers (Multilogin), device-farm operators retool monthly.

**Domain shape.** Per-device-fingerprint signups, accounts-per-device, devices-per-account, per-device login geography spread; per-(fingerprint × asset) cross-asset reuse detection. **Cardinality of device fingerprints ~10^9 globally** drives a different state/storage shape than #2.

**Buyer.** Device-intel vendors building velocity signals on top of their fingerprinting SDK: Castle, Fingerprint, TrustDecision, Incognia, Sardine, Persona, Sumsub, Forter Device, IPQS, Sift Device.

**Scale signals.** Fingerprint >1B identifications/month public figure; Sumsub × Fingerprint partnership June 2025.

**Regulatory.** GDPR / ePrivacy / EDPB 2/2023 + ICO 2025 fingerprinting position — cookie rules apply to alternative fingerprinting (Pinsent Masons).

→ [`results/20_device_intelligence.json`](results/20_device_intelligence.json)

---

### <a id="21-marketplace-matching"></a>21 — On-demand marketplace matching & pricing

**Adversarial pressure:** Low to moderate (incentive gaming).

**Domain shape.** Per-supplier (clinician/driver/contractor) match-history features (acceptance, fill, cancellation, on-time rates), per-(skill, zone) demand counters, per-customer reliability features feeding real-time matching + dynamic pricing.

**Direct comparable.** Chalk × Medely (healthcare staffing): real-time per-clinician matching + dynamic charge-rate pricing; replaced 24-hour batch Redis lag; ~$800k annual revenue lift.

**Mobility/dispatch incumbents.** Uber Gairos, Michelangelo/Palette (5k models, 10M predictions/s), Lyft OSV RL matching ($30M+/yr lift, INFORMS paper), DoorDash DeepRed + Riviera, Instacart Griffin 2.0.

**Freight.** Convoy Platform acquired by DAT ~$250M Jul 2025; Uber Freight rate-to-market FTA predictor.

**Markets.** US healthcare staffing $21–45B (2025); per-diem nursing $10.14B → $13.59B (2030). EU Platform Work Directive 2024/2831 (in force Dec 2024, transpose by Dec 2026).

→ [`results/21_marketplace_matching.json`](results/21_marketplace_matching.json)

---

### <a id="22-betting"></a>22 — Live sports betting risk & odds management

**Adversarial pressure:** Maximum-adjacent. Sharps + syndicates (steam-move coordination), arbers, latency-arbitrage bots, match-fixers.

**Throughput / latency anchors.** Super Bowl 2025 GeoComply peak ~15k tx/s; FanDuel handled 16.6M bets on Super Bowl Sunday 2025; OddsMatrix 1.5M+ bets/day; odds feeds publish ~1M odds/sec. TxODDS Tx Fusion 8–10 ms odds delivery; competitive benchmark sub-200 ms; 3–8 s bet-delay spool is industry standard.

**Market.** Global sports betting $100.9B (2024) → $187.39B (2030) at ~11% CAGR. US handle $166.94B / GGR $16.96B in 2025.

**Incumbents.** DraftKings uses Aerospike + Redis; FanDuel uses Confluent + Flink + Tinybird; bet365 uses Push Diffusion; Kambi runs Tzeract AI trading; Sportradar runs UFDS integrity monitoring.

**Key feature primitives.** Per-(event, market, selection) liability; per-account CLV / stake-factor cascade (50%→25%→10%→1%); per-(market, time_bucket) steam-money; per-account RG/AML velocity; per-(device_fp, ip_hash) collusion fan-out.

**Domain-unique gaps.** Triggers (suspend market when liability_cap exceeded), strict-monetary audit-grade documentation, multi-tenancy for B2B vendors.

→ [`results/22_live_sports_betting.json`](results/22_live_sports_betting.json)

---

### <a id="23-crypto-wallet"></a>23 — Crypto / web3 wallet risk & on-chain monitoring

**Adversarial pressure:** Very high. State-actor laundering (Lazarus, $1.5B Bybit hack Feb 2025), peel-chains, Tornado/Sinbad mixers.

**Latency budget.** TRM Wallet Screening <300 ms (Jun 2025), Elliptic Holistic sub-second; beava sub-ms is an order of magnitude inside.

**Throughput / state.** Top exchanges run thousands-to-tens-of-thousands EPS; Solidus Labs HALO 1T+ events/day. Hardest gap: address-grain cardinality 100M–1B+ exceeds single-node RAM for top-tier exchanges. **Sharding + tiered storage are the dominant production gap.**

**Market & enforcement.** Crypto AML ~$1.2B (2025) → $4.8B (2034), 16.5% CAGR. Enforcement accelerating: OKX $504M Feb 2025; Paxful $3.5M Dec 2025. EU MiCA in force Dec 30 2024; EU TFR no-minimum-threshold for CASP-to-CASP; FATF Travel Rule in 99 jurisdictions.

**Incumbents.** Chainalysis (9 of top-10 exchanges, >1B addresses), TRM Labs ($1B valuation, $220M raised), Elliptic (2B+ labeled addresses, 2M+ screenings/month), Solidus Labs HALO (1T+ events/day), Merkle Science, Crystal, Scorechain.

→ [`results/23_crypto_wallet_risk.json`](results/23_crypto_wallet_risk.json)

---

### <a id="23b-crypto-withdrawal"></a>23b — Crypto-exchange withdrawal-velocity governance

**Adversarial pressure:** Very high, fast-tempo. DPRK / Lazarus, APT actors, supply-chain attacks (Safe{Wallet} 2025).

**Trigger.** 2024–2025 incident wave: DMM $305M, ByBit $1.4–1.5B, WazirX $235M, Indodax $22M, CoinDCX $44M, Nobitex $90M.

**Distinct from #23.** Focus is post-credential-compromise blast-radius controls (per-(account, coin) and (account, counterparty) multi-window counters, new-counterparty velocity, drain-burst features, z-score-vs-envelope), not address taint.

**Single highest-leverage gap.** **Triggers / threshold-cross webhooks** — withdrawal-velocity governance is fundamentally event-driven ("freeze when sum_5m > k") and beava today only serves features. Without triggers, beava slots underneath an existing rules engine (Fireblocks Policy Engine, Sardine, Sift, ComplyAdvantage), not standalone.

**Other production blockers.** TLS / mTLS, RBAC + per-tenant tokens, hot-standby replication, point-in-time reads, idempotency keys, deterministic WAL replay with hashes, native KYT (Chainalysis / TRM / Elliptic) hook, data-residency.

**Urgency.** MiCA CASP regime fully applicable from Dec 2024. Insurance carriers (Coincover, Evertas, Munich Re) require demonstrable real-time withdrawal-velocity controls.

→ [`results/23b_crypto_exchange_withdrawal.json`](results/23b_crypto_exchange_withdrawal.json)

---

### <a id="24-bandit"></a>24 — Bandit / contextual-bandit & online RL serving features

**Adversarial pressure:** Mostly passive load, with a notable exception (adversarial bandits in ranking under integrity pressure).

**Domain shape.** Per-arm rolling reward features (CTR, conversion rate, avg reward), per-(context × arm) interactions, per-policy update counters, per-user exploration-exposure features. **Compound-key atomic counters** = a near-perfect match for `@bv.table(key=…)`.

**2025 commercial signal is strong.** Datadog acquired Eppo $220M May 2025; Optimizely Agent 4.3 (Nov 2025) shipped production CMAB with Redis caching; Spotify deployed neural CMAB on Home page (Mar 2025); Apple Two-Layer Bandit (ICML Jul 2025) doubled engagement in production; Microsoft Personalizer (the canonical "real-world RL service") retires October 2026 — a documented gap.

**Canonical incumbent.** Redis + Lua. Optimizely Agent and Seldon Core both document Redis as the multi-instance bandit-state store. beava differentiator: native compound-key atomic increments, event-id idempotency, no Kafka requirement.

**Gaps.** TLS/auth, multi-tenancy, published p99 `batch_get` benchmark at (1 user × K arms × ~10 features) Rank shape. **No bandit demo in repo** — closest skeleton is the ad-tech demo.

→ [`results/24_bandit_online_rl.json`](results/24_bandit_online_rl.json)

---

### <a id="25-telecom"></a>25 — Telecom CDR fraud (wholesale / IRSF / bypass)

**Adversarial pressure:** High / active. IRSF rings rotate A-numbers and B-prefixes within hours; SIM-box operators retool weekly.

**Domain shape.** Per-A-number, per-B-number, per-destination CDR velocity in ms; IRSF detection; SIM-box / bypass fraud signatures; subscription-fraud. NDSS-2021 IRSF paper: 98% accuracy / 0.28% FPR using exactly the streaming-feature shape beava ships.

**Market.** $38.95B global telecom fraud in 2023 (CFCA, +12% YoY); $6.23B IRSF alone; fraud-management software $5.7–7.6B with 10–12% CAGR. CPaaS AIT alone $1.15B in OTP-delivery costs in 2023; X attributed >$60M to SMS pumping in 2022.

**Highest-impact missing feature.** Triggers / event-driven actions ("when counter crosses threshold, fire SIP-redirect / SMSC-drop"). The difference between hour-batch detection and sub-second blocking is direct interconnect-settlement P&L.

**Other gaps.** TLS+auth (network-segmentation audits), in-process sharding for tier-1 scale, multi-tenancy for CPaaS / wholesale, CDR-format adapters (3GPP 32.298 ASN.1, RADIUS-Accounting, SBC syslog), GLF/IPRN hot-list ingest helper, graph traversal for IRSF chain-unrolling.

**Positioning.** beava sits *under* domain incumbents (Subex, Mobileum, LATRO, Neural Technologies, Hiya, BICS, NetCracker, Symsoft), not against them.

→ [`results/25_telecom_cdr_fraud.json`](results/25_telecom_cdr_fraud.json)

---

### <a id="26-insurance"></a>26 — Real-time insurance underwriting & claims fraud at FNOL

**Adversarial pressure:** High and rising. AI-driven application fraud (ghost brokering, synthetic identities, VIN cloning, address spoofing).

**Two distinct surfaces.** Quote/bind underwriting (200ms–2s budget) and FNOL claim decisioning (seconds-to-minutes with sub-ms inner feature reads). Behavioral session telemetry + telematics + photo/voice/keystroke aggregates. Wide composite entity grain: policyholder, device, ip, vin, property_address, agent, repair_shop, medical_provider.

**Market & buyers.** $308.6B US fraud baseline (CAIF); insurance-fraud-detection software ~$6.6–7.2B in 2025 → $16.9B by 2034; agentic-AI-in-insurance ~$5.76B → $7.26B in 2026. Named buyers: Lemonade, Hippo, Root, Allstate, Progressive, USAA, AXA (Shift Claims). OEM substrate plays under FRISS, Shift Technology, Charlee.ai, Carpe Data.

**Regulation.** NAIC AI Model Bulletin (Dec 2023, 24+ states substantially adopted by Aug 2025); EU AI Act (high-risk); Solvency II; state-DFS; Colorado SB21-169; HIPAA — all require **reproducible `features_snapshot` tied to decisions**.

**15 enumerated gaps.** TLS/auth, multi-tenancy, point-in-time coherent multi-grain snapshot, `features_snapshot_id` primitive, triggers, in-process sharding, cross-region replication, secondary indexes, HLL/CMS/t-digest docs, image/voice perceptual-hash primitives, Guidewire/Duck Creek adapters, insurance demo in `python/beava/demos/`.

→ [`results/26_insurance_underwriting_claims.json`](results/26_insurance_underwriting_claims.json)

---

### <a id="27-supply-chain"></a>27 — Supply chain / inventory real-time stock & velocity

**Adversarial pressure:** Mostly passive load.

**Domain shape.** Per-(sku, store_id), (sku, dc_id), (sku, channel), (supplier_id, sku), (carrier_id, lane_id) primitives. Sell-through, days-of-cover, reservation pressure, stock-out velocity, supplier lead-time percentiles, ETA-deviation, attach-rate — expressible as @bv.table over 5m/15m/1h/24h/7d/30d/90d sliding windows.

**Pull evidence.** Walmart's Kafka-based real-time inventory and replenishment architecture (4,700+ stores, 100M SKUs, ~500M events/day, 4B-message <3h replenishment cycles); Instacart's Confluent + Griffin item-availability inference across 80,000 stores and 100M+ items; project44 and FourKites real-time transportation visibility (1,400+ telematics integrations); Manhattan Associates >$1B annual revenue in 2024.

**Market.** OMS $3–7B in 2024–2026 → $13B by 2035; DOM subsegment $1.5B (2026) → $3.8B (2033) at 10.8% CAGR; WMS $2.8–4.4B (2026) → $10.6B (2034).

**Top gaps (in order).** TLS+auth (universal blocker), triggers (auto-reorder / auto-substitute / auto-rebalance), atomic compare-and-set / reserve-up-to-N for oversell prevention, multi-tenancy for SaaS OMS/WMS vendors, in-process sharding for Tier-1 retailer scale, time-travel/backfill for lead-time-prediction model training, Kafka / EDI / SAP-event-mesh / Manhattan / Blue Yonder / project44 ingest patterns.

→ [`results/27_supply_chain_inventory.json`](results/27_supply_chain_inventory.json)

---

## Cross-cutting themes

### Universal blockers ranked by recurrence in the 38 reports

| Gap | Items where it's blocking | Notes |
|---|---|---|
| **TLS + auth** | All 38 | Universal. Mentioned in every single item summary. Highest-priority v0 → v1 work. |
| **Triggers / fire-on-threshold-cross** | 1a, 1b, 1c, 4, 7b, 9, 9b, 10, 11, 11b, 12, 13, 14b, 15b, 16, 17, 22, 23b, 24, 25, 26, 27 | The single missing primitive that turns beava from feature store into event-driven decisioning. Highest-leverage *new* primitive to ship. |
| **In-process sharding** | 1b (tier-1), 4 (global exchanges), 6/18b (Whatnot+), 9b, 13 (national fleets), 16 (LLM-gateway scale), 22 (tier-1 sportsbook), 23 (top-tier exchange), 26 (top-5 P&C), 27 (Tier-1 retailer) | Caps deployment at mid-market. Bigger urgency in some domains than others. |
| **Multi-tenancy / namespace ACLs** | All vendor-embedded plays: 9, 9b, 11, 11b, 14b, 15b, 15c, 16, 18a, 18b, 20, 26, 27 | Required for the SaaS-embed business model (which seems to be most domains). |
| **Point-in-time / `features_snapshot_id`** | 1c, 7, 8, 18a, 18b, 26 | Required for any regulated decisioning + offline/online parity. Domain-defining gap for credit, AML, insurance. |
| **HLL / CMS / t-digest as first-class** | 1a, 4, 5, 9, 9b, 14, 14b | Mentioned but not surfaced as documented primitives. Cardinality and quantile shapes need this. |

### Demos that would unlock multiple domains at once

| Demo | Items it unlocks |
|---|---|
| `demos/impossible_travel/` | 19 (primary), 1a (ATO), 2, 7, 19, 20 |
| `demos/billing_meter/` | 14b (primary), 14, 15c, 16 |
| `demos/edge_rate_limit/` | 15c (primary), 15b, 16 |
| `demos/llm_gateway/` | 16 (primary), 14b |
| `demos/aml/` | 8 (primary), 1b, 1c, 11b, 23, 23b |
| `demos/recsys_ranking/` | 6 (primary), 18b |
| `demos/anti_cheat/` | 11 (primary), 11b |
| `demos/dispatch_h3/` | 12b (primary), 12, 21 |

### Buyer-persona clusters (for GTM segmentation)

- **Risk / fraud-ops team:** 1a, 1b, 1c, 7, 8, 18a, 22, 25, 26
- **Platform engineering / infra:** 2, 14, 15b, 15c, 24
- **SecOps / detection engineering:** 15, 19, 20, 23b
- **ML platform / personalization-infra:** 6, 18b, 24
- **Vendor-embedded (T&S / fraud / device-intel / billing):** 9, 9b, 10, 11, 11b, 14b, 20, 21, 26
- **Edge / API-gateway / AI-gateway:** 15c, 16
- **Ad-tech ops (DSP/SSP/MMP):** 3, 4
- **Lifecycle/growth/marketing:** 5, 17
- **Operations / fulfillment:** 12, 12b, 13, 27

### "Where this research validates the README pitch"

The 5 README-stated targets (fraud detection, ad-tech, behavioral analytics, broken into 1a/1b/1c/2/3/4/5) all came back as strong technical fits. The expansion into adjacencies (items 6–27) shows that the **same primitives** credibly serve another 20+ domains; the constraint is not feature surface, it's the **production-readiness gaps** above.

The single most under-sold differentiator in the current README is beava's **`distance` / geo-velocity primitive family** — unique in this space and the headline of the impossible-travel demo opportunity (#19).
