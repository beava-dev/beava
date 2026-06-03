import { useCallback } from "react"

import type { DashboardGetQuery } from "@/lib/dashboard-query-types"
import { getFeatureRow, type FeatureRow } from "@/lib/data-api"
import { usePollingResource } from "@/hooks/use-polling-resource"

export type FeatureRowResult = {
  query: DashboardGetQuery
  row: FeatureRow
}

const FEATURE_POLL_MS = 5_000

async function fetchFeatureRows(
  queries: DashboardGetQuery[]
): Promise<FeatureRowResult[]> {
  if (queries.length === 0) {
    return []
  }

  return Promise.all(
    queries.map(async (query) => {
      const row = await getFeatureRow({
        table: query.table,
        key: query.key,
        features: query.features,
      })

      return { query, row }
    })
  )
}

function serializeQueries(queries: DashboardGetQuery[]) {
  return JSON.stringify(queries)
}

export function useFeatureRows(queries: DashboardGetQuery[]) {
  const queryPlanKey = serializeQueries(queries)

  const queryFn = useCallback(
    () => fetchFeatureRows(queries),
    [queryPlanKey, queries]
  )

  return usePollingResource(queryFn, FEATURE_POLL_MS)
}
