import { CollapsibleSection } from "@/components/collapsible-section"
import { PollingStatusBadge } from "@/components/status-badge"
import { MetricsPrometheusPanel } from "@/components/metrics-prometheus-panel"
import { StatCard } from "@/components/stat-card"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import type { MetricsSnapshot } from "@/hooks/use-metrics"
import { useMetrics } from "@/hooks/use-metrics"
import { rssCompositionHints } from "@/lib/memory-profile"
import {
  formatBytes,
  formatCounterRate,
  formatDurationSeconds,
  formatMetricTotal,
} from "@/lib/format-metric"
import type { PollingResource } from "@/hooks/use-polling-resource"

function snapshotLabel(bytes: number | undefined): string {
  if (bytes === undefined || bytes === 0) {
    return "No snapshot yet"
  }

  return formatBytes(bytes)
}

function MetricsSkeleton() {
  return (
    <div className="space-y-4">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 10 }).map((_, index) => (
          <Skeleton key={index} className="h-20 w-full" />
        ))}
      </div>
      <Skeleton className="h-40 w-full" />
    </div>
  )
}

function MetricsBody({ snapshot }: { snapshot: MetricsSnapshot }) {
  const { metrics, rates, rss } = snapshot
  const rssHints = rssCompositionHints(rss)

  return (
    <div className="space-y-6">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          label="Resident entities"
          value={formatMetricTotal(metrics.entityCountResident)}
        />
        <StatCard
          label="Process RSS (profiler)"
          value={
            rss.processResidentBytes === undefined
              ? "—"
              : formatBytes(rss.processResidentBytes)
          }
          hint={rssHints.processRss}
        />
        <StatCard
          label="RSS / resident entity"
          value={
            rss.bytesPerEntityRss === undefined
              ? "—"
              : formatBytes(rss.bytesPerEntityRss)
          }
          hint={rssHints.perEntity}
        />
        <StatCard
          label="Static budget estimate"
          value={
            rss.staticBudgetTotalBytes === undefined
              ? "—"
              : formatBytes(rss.staticBudgetTotalBytes)
          }
          hint={rssHints.staticBudget}
        />
        <StatCard
          label="Registry version"
          value={formatMetricTotal(metrics.registryVersion)}
          hint={`data-plane ping: ${formatMetricTotal(snapshot.dataPlaneRegistryVersion)}`}
        />
        <StatCard
          label="Registered nodes"
          value={formatMetricTotal(metrics.nodeCount ?? snapshot.nodeCount)}
          hint="events + tables + derivations"
        />
        <StatCard
          label="Last snapshot"
          value={snapshotLabel(metrics.snapshotLastBytes)}
          hint={[
            metrics.snapshotLastDurationSeconds !== undefined
              ? `write ${formatDurationSeconds(metrics.snapshotLastDurationSeconds)}`
              : null,
            metrics.snapshotLastFsyncSeconds !== undefined
              ? `fsync ${formatDurationSeconds(metrics.snapshotLastFsyncSeconds)}`
              : null,
            metrics.snapshotLastBytes === undefined ||
            metrics.snapshotLastBytes === 0
              ? "run without --memory-only for disk snapshots"
              : null,
          ]
            .filter(Boolean)
            .join(" · ")}
        />
        <StatCard
          label="Cold evictions"
          value={formatMetricTotal(metrics.coldEntityEvictionsTotal)}
          hint={`rate ${formatCounterRate(
            rates.beava_cold_entity_evictions_total,
            metrics.coldEntityEvictionsTotal
          )}`}
        />
        <StatCard
          label="Bucket reclaims"
          value={formatMetricTotal(metrics.bucketReclaimTotal)}
          hint={`rate ${formatCounterRate(
            rates.beava_bucket_reclaim_total,
            metrics.bucketReclaimTotal
          )}`}
        />
        <StatCard
          label="Cap hits"
          value={formatMetricTotal(metrics.lifetimeOpCapHitTotal)}
          hint={`rate ${formatCounterRate(
            rates.beava_lifetime_op_cap_hit_total,
            metrics.lifetimeOpCapHitTotal
          )}`}
        />
        <StatCard
          label="Entropy capped"
          value={formatMetricTotal(metrics.entropyCategoriesCappedTotal)}
          hint={`rate ${formatCounterRate(
            rates.beava_entropy_categories_capped_total,
            metrics.entropyCategoriesCappedTotal
          )}`}
        />
      </div>

      <CollapsibleSection
        variant="panel"
        title={
          <>
            Admin Prometheus (<code className="text-xs">beava_*</code>)
          </>
        }
        description="Parsed samples from the latest admin /metrics scrape."
      >
        <MetricsPrometheusPanel rows={snapshot.prometheusRows} />
      </CollapsibleSection>

      <CollapsibleSection
        variant="panel"
        title="Full raw exposition"
        description="Unparsed Prometheus text from admin /metrics."
      >
        <pre className="max-h-64 overflow-auto font-mono text-xs text-muted-foreground">
          {snapshot.rawText}
        </pre>
      </CollapsibleSection>
    </div>
  )
}

function MetricsOverviewCard({
  resource,
}: {
  resource: PollingResource<MetricsSnapshot>
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="sr-only">Server metrics</CardTitle>
        <CardDescription>
          Process gauges from admin /metrics plus an admin-side RSS profiler
          (not beava-server). Counter rates need two polls (~5s apart) under
          load.
        </CardDescription>
        <CardAction>
          <PollingStatusBadge resource={resource} />
        </CardAction>
      </CardHeader>
      <CardContent>
        {resource.isLoading && <MetricsSkeleton />}
        {resource.isError && (
          <p className="text-sm text-destructive">
            Unable to fetch metrics from the Beava admin endpoint.
            {resource.error?.message ? (
              <span className="mt-1 block font-mono text-xs text-muted-foreground">
                {resource.error.message}
              </span>
            ) : null}
          </p>
        )}
        {resource.isSuccess && resource.data !== undefined ? (
          <MetricsBody snapshot={resource.data} />
        ) : null}
      </CardContent>
    </Card>
  )
}

export function MetricsOverview() {
  const metrics = useMetrics()

  return <MetricsOverviewCard resource={metrics} />
}
