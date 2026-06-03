import { useCallback, useRef } from "react"

import {
  BEAVA_COUNTER_METRICS,
  counterValue,
  extractBeavaMetrics,
  samplesToRows,
  type BeavaCounterMetric,
  type BeavaMetrics,
} from "@/lib/beava-metrics"
import { getMetricsText } from "@/lib/admin-api"
import {
  buildRssMemoryEstimate,
  fetchMemoryProfile,
  type RssMemoryEstimate,
} from "@/lib/memory-profile"
import { postPing } from "@/lib/data-api"
import { fetchRegistryOverview } from "@/lib/registry-overview"
import { parsePrometheusText } from "@/lib/parse-prometheus"
import { usePollingResource } from "@/hooks/use-polling-resource"

export type CounterRates = Partial<Record<BeavaCounterMetric, number>>

export type MetricsSnapshot = {
  metrics: BeavaMetrics
  rates: CounterRates
  rss: RssMemoryEstimate
  rawText: string
  prometheusRows: ReturnType<typeof samplesToRows>
  nodeCount: number
  dataPlaneRegistryVersion: number
}

const METRICS_POLL_MS = 5_000

function computeRates(
  previous: BeavaMetrics | undefined,
  previousAt: number | undefined,
  current: BeavaMetrics,
  now: number
): CounterRates {
  if (previous === undefined || previousAt === undefined) {
    return {}
  }

  const elapsedSeconds = (now - previousAt) / 1000
  if (elapsedSeconds <= 0) {
    return {}
  }

  const rates: CounterRates = {}

  for (const name of BEAVA_COUNTER_METRICS) {
    const currentValue = counterValue(current, name)
    const previousValue = counterValue(previous, name)

    if (currentValue === undefined || previousValue === undefined) {
      continue
    }

    const delta = currentValue - previousValue
    if (delta >= 0) {
      rates[name] = delta / elapsedSeconds
    }
  }

  return rates
}

function normalizeMetrics(
  metrics: BeavaMetrics,
  pingVersion: number,
  nodeCount: number
): BeavaMetrics {
  let next = { ...metrics }

  if (
    pingVersion > 0 &&
    (next.registryVersion === undefined || next.registryVersion === 0)
  ) {
    next = { ...next, registryVersion: pingVersion }
  }

  if (
    nodeCount > 0 &&
    (next.nodeCount === undefined || next.nodeCount === 0)
  ) {
    next = { ...next, nodeCount }
  }

  const counterDefaults: Array<keyof BeavaMetrics> = [
    "entropyCategoriesCappedTotal",
    "coldEntityEvictionsTotal",
    "lifetimeOpCapHitTotal",
    "bucketReclaimTotal",
  ]

  for (const key of counterDefaults) {
    if (next[key] === undefined) {
      next = { ...next, [key]: 0 }
    }
  }

  return next
}

async function fetchMemoryProfileSafe() {
  try {
    return await fetchMemoryProfile()
  } catch {
    return {
      source: "unavailable" as const,
      detail:
        "Start server/memory-profile.mjs (see .env.example) or use compose with docker.sock.",
    }
  }
}

async function fetchMetricsSnapshot(
  previous: BeavaMetrics | undefined,
  previousAt: number | undefined
): Promise<MetricsSnapshot> {
  const [rawText, ping, registry, memoryProfile] = await Promise.all([
    getMetricsText(),
    postPing(),
    fetchRegistryOverview(),
    fetchMemoryProfileSafe(),
  ])
  const samples = parsePrometheusText(rawText)
  const metrics = normalizeMetrics(
    extractBeavaMetrics(samples),
    ping.registry_version,
    registry.node_count
  )
  const rss = buildRssMemoryEstimate(
    memoryProfile,
    metrics.entityCountResident,
    metrics.bytesPerEntityP99
  )

  const now = Date.now()
  const rates = computeRates(previous, previousAt, metrics, now)
  const prometheusRows = samplesToRows(samples).filter((row) =>
    row.name.startsWith("beava_")
  )

  return {
    metrics,
    rates,
    rss,
    rawText,
    prometheusRows,
    nodeCount: registry.node_count,
    dataPlaneRegistryVersion: ping.registry_version,
  }
}

export function useMetrics() {
  const previousRef = useRef<{
    metrics: BeavaMetrics
    at: number
  }>()

  const queryFn = useCallback(async () => {
    const previous = previousRef.current
    const snapshot = await fetchMetricsSnapshot(
      previous?.metrics,
      previous?.at
    )
    previousRef.current = { metrics: snapshot.metrics, at: Date.now() }
    return snapshot
  }, [])

  return usePollingResource(queryFn, METRICS_POLL_MS)
}
