export type DashboardGetQuery = {
  table: string
  key: string
  label?: string
  /** Optional POST /get projection; omit to return all columns on the row. */
  features?: string[]
}

export type DashboardProfileId = "bench-small" | "bench-medium" | "registry"

export type QueryableTarget = {
  table: string
  primaryKeyFields: string[]
  isGlobal: boolean
}
