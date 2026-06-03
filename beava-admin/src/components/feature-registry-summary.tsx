import { CollapsibleSection } from "@/components/collapsible-section"
import { MetricRow } from "@/components/metric-row"
import { SuccessBadge } from "@/components/status-badge"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import type { DashboardQueriesState } from "@/hooks/use-dashboard-queries"
import { catalogFromDump, tableOutputFields } from "@/lib/registry-catalog"

function NameList({ names }: { names: string[] }) {
  if (names.length === 0) {
    return <span className="text-muted-foreground">(none)</span>
  }

  return (
    <span className="font-mono text-xs text-foreground">{names.join(", ")}</span>
  )
}

type FeatureRegistrySummaryProps = {
  dashboard: DashboardQueriesState
}

export function FeatureRegistrySummary({ dashboard }: FeatureRegistrySummaryProps) {
  const dump = dashboard.registryDump
  const activeTable = dashboard.queries[0]?.table

  return (
    <CollapsibleSection
      title="Registry on server"
      description="From data-plane GET /registry when dev endpoints are enabled."
      action={
        dump ? <SuccessBadge>Live</SuccessBadge> : <Badge variant="secondary">N/A</Badge>
      }
    >
      {!dashboard.registryAvailable ? (
        <p className="text-sm text-muted-foreground">
          Enable <code className="text-xs">dev_endpoints</code> or run with{" "}
          <code className="text-xs">--test-mode</code> to list pipelines here.
        </p>
      ) : dump === undefined ? (
        <Skeleton className="h-16 w-full" />
      ) : (
        <div className="space-y-4">
          <dl>
            <MetricRow label="Registry version" value={dump.version} />
            <MetricRow
              label="Events"
              value={<NameList names={catalogFromDump(dump).events} />}
            />
            <MetricRow
              label="Tables / derivations"
              value={
                <NameList
                  names={[
                    ...catalogFromDump(dump).tables,
                    ...catalogFromDump(dump).derivations,
                  ]}
                />
              }
            />
          </dl>
          {activeTable ? (
            <div className="rounded-md border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
              <span className="font-medium text-foreground">{activeTable}</span>{" "}
              output fields:{" "}
              <code className="text-foreground">
                {tableOutputFields(dump, activeTable).join(", ") || "(unknown)"}
              </code>
              . Bench <code className="text-xs">small</code> only exposes{" "}
              <code className="text-xs">cnt</code> per entity until you register a
              richer pipeline.
            </div>
          ) : null}
        </div>
      )}
    </CollapsibleSection>
  )
}
