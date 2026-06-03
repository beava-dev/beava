import type {
  DashboardGetQuery,
  QueryableTarget,
} from "@/lib/dashboard-query-types"
import type { RegistryDump } from "@/lib/data-api"

function isGlobalPrimaryKey(fields: string[] | undefined): boolean {
  return !fields || fields.length === 0
}

export function listQueryableTargets(dump: RegistryDump): QueryableTarget[] {
  const targets: QueryableTarget[] = []

  for (const [name, raw] of Object.entries(dump.tables)) {
    const desc = raw as { primary_key?: string[] }
    const primaryKeyFields = desc.primary_key ?? []
    targets.push({
      table: name,
      primaryKeyFields,
      isGlobal: isGlobalPrimaryKey(primaryKeyFields),
    })
  }

  for (const [name, raw] of Object.entries(dump.derivations)) {
    const desc = raw as {
      output_kind?: string
      table_primary_key?: string[]
    }
    if (desc.output_kind !== "table") {
      continue
    }

    const primaryKeyFields = desc.table_primary_key ?? []
    targets.push({
      table: name,
      primaryKeyFields,
      isGlobal: isGlobalPrimaryKey(primaryKeyFields),
    })
  }

  return targets
}

export function planQueriesFromRegistry(
  dump: RegistryDump,
  sampleKeys: Record<string, string>
): { queries: DashboardGetQuery[]; missingKeys: QueryableTarget[] } {
  const targets = listQueryableTargets(dump)
  const queries: DashboardGetQuery[] = []
  const missingKeys: QueryableTarget[] = []

  for (const target of targets) {
    if (target.isGlobal) {
      queries.push({ table: target.table, key: "", label: target.table })
      continue
    }

    const key = sampleKeys[target.table]
    if (!key) {
      missingKeys.push(target)
      continue
    }

    queries.push({
      table: target.table,
      key,
      label: target.table,
    })
  }

  return { queries, missingKeys }
}
