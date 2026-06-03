import { CollapsibleSection } from "@/components/collapsible-section"
import { DebugCommandsPanel } from "@/components/debug/debug-commands-panel"
import { DebugMetricsTable } from "@/components/debug/debug-metrics-table"
import { DebugProbeTable } from "@/components/debug/debug-probe-table"
import { DebugRawProbes } from "@/components/debug/debug-raw-probes"
import { DebugRegistryPanel } from "@/components/debug/debug-registry-panel"
import { DebugTargets } from "@/components/debug/debug-targets"
import { PollingStatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import type { DebugSnapshot } from "@/lib/debug-probes"
import type { PollingResource } from "@/hooks/use-polling-resource"

type DebugOverviewProps = {
  resource: PollingResource<DebugSnapshot>
}

function DebugSkeleton() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-32 w-full" />
      <Skeleton className="h-48 w-full" />
      <Skeleton className="h-64 w-full" />
    </div>
  )
}

function DebugBody({
  snapshot,
  onRefresh,
}: {
  snapshot: DebugSnapshot
  onRefresh: () => void
}) {
  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm text-muted-foreground">
        <span>Captured {new Date(snapshot.capturedAt).toLocaleString()}</span>
        <Button type="button" variant="outline" size="sm" onClick={onRefresh}>
          Refresh now
        </Button>
      </div>

      <CollapsibleSection title="Targets" description="Admin and data-plane URLs from env.">
        <DebugTargets />
      </CollapsibleSection>

      <DebugCommandsPanel onCommandComplete={onRefresh} />

      <Card>
        <CardHeader>
          <CardTitle>Endpoint probes</CardTitle>
          <CardDescription>
            Parallel requests to admin and data planes. X-Runtime distinguishes
            tokio (admin) from mio (data).
          </CardDescription>
        </CardHeader>
        <CardContent>
          <DebugProbeTable probes={snapshot.probes} />
        </CardContent>
      </Card>

      <CollapsibleSection
        title="Registry cross-check"
        description="Admin vs data-plane registry versions and pipeline lists."
      >
        <DebugRegistryPanel snapshot={snapshot} />
      </CollapsibleSection>

      <CollapsibleSection
        title="Prometheus samples"
        description="Parsed beava_* metrics from the admin /metrics probe."
      >
        <DebugMetricsTable rows={snapshot.metricRows} />
      </CollapsibleSection>

      <CollapsibleSection
        title="Raw responses"
        description="Full bodies for each probe (JSON or Prometheus text)."
      >
        <DebugRawProbes probes={snapshot.probes} />
      </CollapsibleSection>
    </div>
  )
}

export function DebugOverview({ resource }: DebugOverviewProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="sr-only">Debug snapshot</CardTitle>
        <CardAction className="flex items-center gap-2">
          <PollingStatusBadge resource={resource} />
        </CardAction>
      </CardHeader>
      <CardContent>
        {resource.isLoading && <DebugSkeleton />}
        {resource.isError && (
          <p className="text-sm text-destructive">
            Debug snapshot failed.
            {resource.error?.message ? (
              <span className="mt-1 block font-mono text-xs text-muted-foreground">
                {resource.error.message}
              </span>
            ) : null}
          </p>
        )}
        {resource.isSuccess && resource.data !== undefined ? (
          <DebugBody
            snapshot={resource.data}
            onRefresh={() => {
              void resource.refetch()
            }}
          />
        ) : null}
      </CardContent>
    </Card>
  )
}
