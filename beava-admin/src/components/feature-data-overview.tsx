import { CollapsibleSection } from "@/components/collapsible-section"
import { MetricRow } from "@/components/metric-row"
import { FeatureQueryToolbar } from "@/components/feature-query-toolbar"
import { FeatureRegistrySummary } from "@/components/feature-registry-summary"
import { PollingStatusBadge } from "@/components/status-badge"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { useDashboardQueries } from "@/hooks/use-dashboard-queries"
import type { FeatureRowResult } from "@/hooks/use-feature-rows"
import { useFeatureRows } from "@/hooks/use-feature-rows"
import type { PollingResource } from "@/hooks/use-polling-resource"
import type { FeatureRow } from "@/lib/data-api"
import { tableOutputFields } from "@/lib/registry-catalog"

function formatFeatureValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "—"
  }

  if (typeof value === "object") {
    return JSON.stringify(value)
  }

  return String(value)
}

function FeatureRowList({ row }: { row: FeatureRow }) {
  const entries = Object.entries(row)

  if (entries.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No data yet (cold start). Push events for this table on the data plane.
      </p>
    )
  }

  return (
    <dl>
      {entries.map(([name, value]) => (
        <MetricRow key={name} label={name} value={formatFeatureValue(value)} />
      ))}
    </dl>
  )
}

function FeatureResults({
  results,
  expectedFields,
}: {
  results: FeatureRowResult[]
  expectedFields: string[]
}) {
  return (
    <div className="space-y-6">
      {results.map(({ query, row }) => {
        const returnedFields = Object.keys(row)
        const missingFields = expectedFields.filter(
          (name) => !returnedFields.includes(name)
        )

        return (
          <CollapsibleSection
            key={`${query.table}:${query.key}`}
            variant="panel"
            title={query.label ?? query.table}
            description={`${query.table} · ${query.key || "site-wide"}`}
          >
            <dl className="mb-3 rounded-md border border-border bg-muted/20 px-3 py-2">
              <MetricRow label="Table" value={query.table} />
              <MetricRow
                label="Entity key"
                value={query.key || "(site-wide / empty key)"}
              />
              <MetricRow
                label="Fields returned"
                value={
                  returnedFields.length > 0
                    ? returnedFields.join(", ")
                    : "(none yet)"
                }
              />
            </dl>
            <FeatureRowList row={row} />
            {expectedFields.length > 0 ? (
              <p className="mt-2 text-xs text-muted-foreground">
                Registry schema for this table:{" "}
                <code className="text-foreground">
                  {expectedFields.join(", ")}
                </code>
                {missingFields.length > 0
                  ? `. Missing until data lands: ${missingFields.join(", ")}.`
                  : "."}
              </p>
            ) : null}
          </CollapsibleSection>
        )
      })}
    </div>
  )
}

function FeatureDataSkeleton() {
  return (
    <div className="space-y-3">
      <Skeleton className="h-4 w-2/3" />
      <Skeleton className="h-4 w-1/2" />
      <Skeleton className="h-4 w-3/5" />
    </div>
  )
}

function FeatureDataError({ error }: { error: Error }) {
  return (
    <p className="text-sm text-destructive">
      Unable to query feature rows from the Beava data plane.
      {error.message ? (
        <span className="mt-1 block font-mono text-xs text-muted-foreground">
          {error.message}
        </span>
      ) : null}
    </p>
  )
}

function FeatureDataCard({
  resource,
  emptyMessage,
  expectedFields,
}: {
  resource: PollingResource<FeatureRowResult[]>
  emptyMessage?: string
  expectedFields: string[]
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="sr-only">Feature data</CardTitle>
        <CardDescription>
          Live rows from POST /get. Pick a profile or let the registry profile
          plan queries; override with env vars when deploying.
        </CardDescription>
        <CardAction>
          <PollingStatusBadge resource={resource} />
        </CardAction>
      </CardHeader>
      <CardContent>
        {emptyMessage ? (
          <p className="mb-4 text-sm text-muted-foreground">{emptyMessage}</p>
        ) : null}
        {resource.isLoading && <FeatureDataSkeleton />}
        {resource.isError && resource.error ? (
          <FeatureDataError error={resource.error} />
        ) : null}
        {resource.isSuccess && resource.data !== undefined ? (
          <FeatureResults
            results={resource.data}
            expectedFields={expectedFields}
          />
        ) : null}
      </CardContent>
    </Card>
  )
}

export function FeatureDataOverview() {
  const dashboard = useDashboardQueries()
  const featureRows = useFeatureRows(dashboard.queries)
  const activeTable = dashboard.queries[0]?.table
  const registrySchemaFields =
    dashboard.registryDump && activeTable
      ? tableOutputFields(dashboard.registryDump, activeTable)
      : []
  const schemaFields =
    registrySchemaFields.length > 0
      ? registrySchemaFields
      : dashboard.expectedFields

  const emptyMessage =
    dashboard.queries.length === 0
      ? dashboard.profileId === "registry" && !dashboard.registryAvailable
        ? "Registry profile needs GET /registry (dev_endpoints or --test-mode). Pick Bench (medium) or Bench (small)."
        : dashboard.missingKeyedTargets.length > 0
          ? "Enter sample keys above for keyed tables, or switch to Bench (small)."
          : "No /get queries configured for the current profile."
      : undefined

  return (
    <div className="space-y-4">
      <FeatureQueryToolbar dashboard={dashboard} />
      <FeatureRegistrySummary dashboard={dashboard} />
      <FeatureDataCard
        resource={featureRows}
        emptyMessage={emptyMessage}
        expectedFields={schemaFields}
      />
    </div>
  )
}
