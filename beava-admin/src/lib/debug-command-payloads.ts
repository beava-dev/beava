/** Register bodies aligned with crates/beava-bench/configs/{small,medium}.json */

export const benchRegisterSmall = {
  nodes: [
    {
      kind: "event",
      name: "Txn",
      schema: {
        fields: {
          event_time: "i64",
          user_id: "str",
          amount: "f64",
        },
        optional_fields: [] as string[],
      },
    },
    {
      kind: "derivation",
      name: "TxnAgg",
      output_kind: "table",
      upstreams: ["Txn"],
      ops: [
        {
          op: "group_by",
          keys: ["user_id"],
          agg: {
            cnt: { op: "count", params: {} },
          },
        },
      ],
      schema: {
        fields: {
          user_id: "str",
          cnt: "i64",
        },
        optional_fields: [] as string[],
      },
      table_primary_key: ["user_id"],
    },
  ],
} as const

export const benchRegisterMedium = {
  nodes: [
    {
      kind: "event",
      name: "Txn",
      schema: {
        fields: {
          event_time: "i64",
          user_id: "str",
          amount: "f64",
        },
        optional_fields: [] as string[],
      },
    },
    {
      kind: "derivation",
      name: "TxnAgg",
      output_kind: "table",
      upstreams: ["Txn"],
      ops: [
        {
          op: "group_by",
          keys: ["user_id"],
          agg: {
            cnt: { op: "count", params: {} },
            sum_amt: { op: "sum", params: { field: "amount" } },
            avg_amt: { op: "mean", params: { field: "amount" } },
            min_amt: { op: "min", params: { field: "amount" } },
            max_amt: { op: "max", params: { field: "amount" } },
          },
        },
      ],
      schema: {
        fields: {
          user_id: "str",
          cnt: "i64",
          sum_amt: "f64",
          avg_amt: "f64",
          min_amt: "f64",
          max_amt: "f64",
        },
        optional_fields: [] as string[],
      },
      table_primary_key: ["user_id"],
    },
  ],
} as const

export const benchSampleKey = "k00000000"
