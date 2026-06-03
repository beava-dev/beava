import type { DashboardGetQuery, DashboardProfileId } from "@/lib/dashboard-query-types"

export type DashboardProfile = {
  id: DashboardProfileId
  label: string
  description: string
  queries: DashboardGetQuery[]
  /** Schema columns expected for the primary bench table (shown when GET /registry is off). */
  expectedFields?: string[]
  /** Default entity keys for keyed tables (bench / dev). */
  sampleKeys?: Record<string, string>
}

export const DASHBOARD_PROFILES: Record<DashboardProfileId, DashboardProfile> = {
  "bench-small": {
    id: "bench-small",
    label: "Bench (small)",
    description:
      "TxnAgg row for crates/beava-bench configs/small.json (default blast fixed key).",
    expectedFields: ["user_id", "cnt"],
    queries: [
      {
        table: "TxnAgg",
        key: "k00000000",
        label: "TxnAgg (small)",
        features: ["cnt"],
      },
    ],
    sampleKeys: { TxnAgg: "k00000000" },
  },
  "bench-medium": {
    id: "bench-medium",
    label: "Bench (medium)",
    description:
      "TxnAgg with cnt, sum_amt, avg_amt, min_amt, max_amt (configs/medium.json).",
    expectedFields: [
      "user_id",
      "cnt",
      "sum_amt",
      "avg_amt",
      "min_amt",
      "max_amt",
    ],
    queries: [
      {
        table: "TxnAgg",
        key: "k00000000",
        label: "TxnAgg (medium)",
        features: ["cnt", "sum_amt", "avg_amt", "min_amt", "max_amt"],
      },
    ],
    sampleKeys: { TxnAgg: "k00000000" },
  },
  registry: {
    id: "registry",
    label: "From registry",
    description:
      "Plan POST /get targets from data-plane GET /registry (dev_endpoints / test mode).",
    queries: [],
  },
}

export const DEFAULT_PROFILE_ID: DashboardProfileId = "bench-medium"
