import type { RegistryDump } from "@/lib/data-api"

export type RegistryCatalog = {
  version: number
  events: string[]
  tables: string[]
  derivations: string[]
}

export function catalogFromDump(dump: RegistryDump): RegistryCatalog {
  return {
    version: dump.version,
    events: Object.keys(dump.events),
    tables: Object.keys(dump.tables),
    derivations: Object.keys(dump.derivations),
  }
}

export function tableOutputFields(
  dump: RegistryDump,
  tableName: string
): string[] {
  const deriv = dump.derivations[tableName] as
    | { schema?: { fields?: Record<string, unknown> } }
    | undefined

  if (deriv?.schema?.fields) {
    return Object.keys(deriv.schema.fields)
  }

  const table = dump.tables[tableName] as
    | { schema?: { fields?: Record<string, unknown> } }
    | undefined

  if (table?.schema?.fields) {
    return Object.keys(table.schema.fields)
  }

  return []
}
