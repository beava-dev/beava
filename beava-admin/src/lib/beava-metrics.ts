import { findMetric, type PrometheusSample } from "@/lib/parse-prometheus"

export type BeavaMetrics = {
  registryVersion?: number
  nodeCount?: number
  entityCountResident?: number
  bytesPerEntityP99?: number
  snapshotLastDurationSeconds?: number
  snapshotLastBytes?: number
  snapshotLastFsyncSeconds?: number
  entropyCategoriesCappedTotal?: number
  coldEntityEvictionsTotal?: number
  lifetimeOpCapHitTotal?: number
  bucketReclaimTotal?: number
}

export const BEAVA_COUNTER_METRICS = [
  "beava_entropy_categories_capped_total",
  "beava_cold_entity_evictions_total",
  "beava_lifetime_op_cap_hit_total",
  "beava_bucket_reclaim_total",
] as const

export type BeavaCounterMetric = (typeof BEAVA_COUNTER_METRICS)[number]

export function extractBeavaMetrics(samples: PrometheusSample[]): BeavaMetrics {
  return {
    registryVersion: findMetric(samples, "beava_registry_version"),
    nodeCount: findMetric(samples, "beava_node_count"),
    entityCountResident: findMetric(samples, "beava_entity_count_resident"),
    bytesPerEntityP99: findMetric(samples, "beava_bytes_per_entity_p99"),
    snapshotLastDurationSeconds: findMetric(
      samples,
      "beava_snapshot_last_duration_seconds"
    ),
    snapshotLastBytes: findMetric(samples, "beava_snapshot_last_bytes"),
    snapshotLastFsyncSeconds: findMetric(
      samples,
      "beava_snapshot_last_fsync_seconds"
    ),
    entropyCategoriesCappedTotal: findMetric(
      samples,
      "beava_entropy_categories_capped_total"
    ),
    coldEntityEvictionsTotal: findMetric(
      samples,
      "beava_cold_entity_evictions_total"
    ),
    lifetimeOpCapHitTotal: findMetric(samples, "beava_lifetime_op_cap_hit_total"),
    bucketReclaimTotal: findMetric(samples, "beava_bucket_reclaim_total"),
  }
}

export function estimatedResidentBytes(metrics: BeavaMetrics): number | undefined {
  if (
    metrics.entityCountResident === undefined ||
    metrics.bytesPerEntityP99 === undefined
  ) {
    return undefined
  }

  return metrics.entityCountResident * metrics.bytesPerEntityP99
}

export type MetricRow = {
  name: string
  labels: string
  value: number
}

export function samplesToRows(samples: PrometheusSample[]): MetricRow[] {
  return samples.map((sample) => ({
    name: sample.name,
    labels:
      Object.keys(sample.labels).length > 0
        ? Object.entries(sample.labels)
            .map(([key, value]) => `${key}="${value}"`)
            .join(", ")
        : "—",
    value: sample.value,
  }))
}

export function counterValue(
  metrics: BeavaMetrics,
  name: BeavaCounterMetric
): number | undefined {
  switch (name) {
    case "beava_entropy_categories_capped_total":
      return metrics.entropyCategoriesCappedTotal
    case "beava_cold_entity_evictions_total":
      return metrics.coldEntityEvictionsTotal
    case "beava_lifetime_op_cap_hit_total":
      return metrics.lifetimeOpCapHitTotal
    case "beava_bucket_reclaim_total":
      return metrics.bucketReclaimTotal
  }
}
