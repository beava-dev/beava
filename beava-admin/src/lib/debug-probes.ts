import { getEnvDashboardQueries } from "@/lib/dashboard-config"
import { beavaConfig } from "@/lib/config"
import { DASHBOARD_PROFILES } from "@/lib/dashboard-profiles"
import { samplesToRows } from "@/lib/beava-metrics"
import { parsePrometheusText } from "@/lib/parse-prometheus"
import { httpProbe, type HttpProbeResult } from "@/lib/http-probe"

export type RegistryCatalog = {
  version: number
  events: string[]
  tables: string[]
  derivations: string[]
  devOnly: boolean
}

export type DebugSnapshot = {
  capturedAt: string
  probes: HttpProbeResult[]
  metricRows: ReturnType<typeof samplesToRows>
  adminRegistry: { version: number; node_count: number } | undefined
  dataRegistry: RegistryCatalog | undefined
  dataPing: { pong: boolean; registry_version: number } | undefined
  registryMismatch: boolean | undefined
}

function catalogFromDump(dump: {
  version: number
  events: Record<string, unknown>
  tables: Record<string, unknown>
  derivations: Record<string, unknown>
  _dev_only: boolean
}): RegistryCatalog {
  return {
    version: dump.version,
    events: Object.keys(dump.events),
    tables: Object.keys(dump.tables),
    derivations: Object.keys(dump.derivations),
    devOnly: dump._dev_only,
  }
}

function buildProbeRequests(): import("@/lib/http-probe").HttpProbeRequest[] {
  const jsonHeaders = { "content-type": "application/json" }
  const queries =
    getEnvDashboardQueries() ?? DASHBOARD_PROFILES["bench-small"].queries

  const probes: import("@/lib/http-probe").HttpProbeRequest[] = [
    {
      id: "admin-health",
      label: "Admin /health",
      url: `${beavaConfig.adminUrl}/health`,
    },
    {
      id: "admin-ready",
      label: "Admin /ready",
      url: `${beavaConfig.adminUrl}/ready`,
    },
    {
      id: "admin-registry",
      label: "Admin /registry",
      url: `${beavaConfig.adminUrl}/registry`,
    },
    {
      id: "admin-metrics",
      label: "Admin /metrics",
      url: `${beavaConfig.adminUrl}/metrics`,
    },
    {
      id: "data-health",
      label: "Data /health",
      url: `${beavaConfig.dataUrl}/health`,
    },
    {
      id: "data-ready",
      label: "Data /ready",
      url: `${beavaConfig.dataUrl}/ready`,
    },
    {
      id: "data-registry",
      label: "Data /registry (dev)",
      url: `${beavaConfig.dataUrl}/registry`,
    },
    {
      id: "data-ping",
      label: "Data POST /ping",
      method: "POST" as const,
      url: `${beavaConfig.dataUrl}/ping`,
      body: "{}",
      headers: jsonHeaders,
    },
  ]

  for (const query of queries) {
    probes.push({
      id: `data-get-${query.table}-${query.key || "global"}`,
      label: `Data POST /get · ${query.label ?? query.table}`,
      method: "POST" as const,
      url: `${beavaConfig.dataUrl}/get`,
      body: JSON.stringify({
        table: query.table,
        key: query.key,
      }),
      headers: jsonHeaders,
    })
  }

  return probes
}

export async function captureDebugSnapshot(): Promise<DebugSnapshot> {
  const probes = await Promise.all(buildProbeRequests().map(httpProbe))

  let metricRows: DebugSnapshot["metricRows"] = []
  const metricsProbe = probes.find((probe) => probe.id === "admin-metrics")
  if (metricsProbe?.ok) {
    metricRows = samplesToRows(parsePrometheusText(metricsProbe.body))
  }

  const adminRegistryProbe = probes.find((probe) => probe.id === "admin-registry")
  const adminRegistry =
    adminRegistryProbe?.ok &&
    adminRegistryProbe.parsedJson &&
    typeof adminRegistryProbe.parsedJson === "object" &&
    adminRegistryProbe.parsedJson !== null &&
    "version" in adminRegistryProbe.parsedJson &&
    "node_count" in adminRegistryProbe.parsedJson
      ? {
          version: Number(
            (adminRegistryProbe.parsedJson as { version: unknown }).version
          ),
          node_count: Number(
            (adminRegistryProbe.parsedJson as { node_count: unknown }).node_count
          ),
        }
      : undefined

  const dataRegistryProbe = probes.find((probe) => probe.id === "data-registry")
  let dataRegistry: RegistryCatalog | undefined
  if (
    dataRegistryProbe?.ok &&
    dataRegistryProbe.parsedJson &&
    typeof dataRegistryProbe.parsedJson === "object" &&
    dataRegistryProbe.parsedJson !== null &&
    "events" in dataRegistryProbe.parsedJson
  ) {
    dataRegistry = catalogFromDump(
      dataRegistryProbe.parsedJson as Parameters<typeof catalogFromDump>[0]
    )
  }

  const pingProbe = probes.find((probe) => probe.id === "data-ping")
  const dataPing =
    pingProbe?.ok &&
    pingProbe.parsedJson &&
    typeof pingProbe.parsedJson === "object" &&
    pingProbe.parsedJson !== null &&
    "registry_version" in pingProbe.parsedJson
      ? {
          pong: Boolean((pingProbe.parsedJson as { pong?: unknown }).pong),
          registry_version: Number(
            (pingProbe.parsedJson as { registry_version: unknown })
              .registry_version
          ),
        }
      : undefined

  const registryMismatch =
    adminRegistry !== undefined && dataRegistry !== undefined
      ? adminRegistry.version !== dataRegistry.version
      : undefined

  return {
    capturedAt: new Date().toISOString(),
    probes,
    metricRows,
    adminRegistry,
    dataRegistry,
    dataPing,
    registryMismatch,
  }
}
