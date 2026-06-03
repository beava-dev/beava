import { beavaConfig } from "@/lib/config"
import { formatBytes, formatInteger } from "@/lib/format-metric"

export type MemoryProfileSource =
  | "pid"
  | "docker-exec"
  | "docker-stats"
  | "unavailable"

export type MemoryProfileResponse = {
  processResidentBytes?: number
  source: MemoryProfileSource
  detail?: string
  container?: string
  processPid?: string
  hostPid?: string
}

export type RssMemoryEstimate = {
  processResidentBytes?: number
  bytesPerEntityRss?: number
  staticBudgetBytesPerEntity?: number
  staticBudgetTotalBytes?: number
  entityCountResident?: number
  source: MemoryProfileSource
  detail?: string
  profileContainer?: string
  profileProcessPid?: string
  profileHostPid?: string
}

export type RssCompositionHints = {
  processRss: string
  perEntity: string
  staticBudget: string
}

function formatBytesLiteral(value: number): string {
  return `${formatInteger(value)} B`
}

export async function fetchMemoryProfile(): Promise<MemoryProfileResponse> {
  const response = await fetch(`${beavaConfig.adminUrl}/memory-profile`, {
    cache: "no-store",
  })

  if (!response.ok) {
    return {
      source: "unavailable",
      detail: `memory-profile HTTP ${response.status}`,
    }
  }

  const body = (await response.json()) as MemoryProfileResponse
  return {
    processResidentBytes: body.processResidentBytes,
    source: body.source ?? "unavailable",
    detail: body.detail,
    container: body.container,
    processPid: body.processPid,
    hostPid: body.hostPid,
  }
}

export function buildRssMemoryEstimate(
  profile: MemoryProfileResponse,
  entityCountResident: number | undefined,
  staticBudgetBytesPerEntity: number | undefined
): RssMemoryEstimate {
  const processResidentBytes =
    profile.processResidentBytes !== undefined &&
    profile.processResidentBytes > 0
      ? profile.processResidentBytes
      : undefined

  const bytesPerEntityRss =
    processResidentBytes !== undefined &&
    entityCountResident !== undefined &&
    entityCountResident > 0
      ? Math.floor(processResidentBytes / entityCountResident)
      : undefined

  const staticBudgetTotalBytes =
    staticBudgetBytesPerEntity !== undefined &&
    entityCountResident !== undefined &&
    entityCountResident > 0
      ? staticBudgetBytesPerEntity * entityCountResident
      : undefined

  return {
    processResidentBytes,
    bytesPerEntityRss,
    staticBudgetBytesPerEntity,
    staticBudgetTotalBytes,
    entityCountResident,
    source: profile.source,
    detail: profile.detail,
    profileContainer: profile.container,
    profileProcessPid: profile.processPid,
    profileHostPid: profile.hostPid,
  }
}

function profilerSourceLabel(source: MemoryProfileSource): string {
  switch (source) {
    case "pid":
      return "pid (local ps)"
    case "docker-exec":
      return "docker-exec (ps in container)"
    case "docker-stats":
      return "docker-stats (whole container)"
    case "unavailable":
      return "unavailable"
  }
}

export function rssCompositionHints(rss: RssMemoryEstimate): RssCompositionHints {
  const entities = rss.entityCountResident
  const entityLabel =
    entities === undefined ? "— entities" : `${formatInteger(entities)} entities`

  let processRss = profilerSourceLabel(rss.source)
  if (rss.processResidentBytes !== undefined) {
    processRss = `${formatBytesLiteral(rss.processResidentBytes)} (${formatBytes(rss.processResidentBytes)}) · ${profilerSourceLabel(rss.source)}`
    const scope = [
      rss.profileContainer ? rss.profileContainer : null,
      rss.profileProcessPid ? `pid ${rss.profileProcessPid}` : null,
      rss.profileHostPid ? `host pid ${rss.profileHostPid}` : null,
    ]
      .filter(Boolean)
      .join(", ")
    if (scope) {
      processRss = `${processRss} · ${scope}`
    }
  } else if (rss.detail) {
    processRss = `${processRss} · ${rss.detail}`
  }

  let perEntity = "needs profiler RSS and entity count > 0"
  if (
    rss.processResidentBytes !== undefined &&
    entities !== undefined &&
    entities > 0 &&
    rss.bytesPerEntityRss !== undefined
  ) {
    perEntity = `${formatBytesLiteral(rss.processResidentBytes)} ÷ ${formatInteger(entities)} = ${formatBytesLiteral(rss.bytesPerEntityRss)}/entity (${formatBytes(rss.bytesPerEntityRss)}) · includes WAL, threads, overhead`
  } else if (entities === 0) {
    perEntity = `${formatBytesLiteral(rss.processResidentBytes ?? 0)} ÷ 0 entities (undefined)`
  }

  let staticBudget = "needs entity count and beava_bytes_per_entity_p99"
  if (
    rss.staticBudgetBytesPerEntity !== undefined &&
    entities !== undefined &&
    entities > 0 &&
    rss.staticBudgetTotalBytes !== undefined
  ) {
    staticBudget = `${formatBytesLiteral(rss.staticBudgetBytesPerEntity)} × ${formatInteger(entities)} = ${formatBytesLiteral(rss.staticBudgetTotalBytes)} (${formatBytes(rss.staticBudgetTotalBytes)}) · prometheus placeholder`
  } else if (rss.staticBudgetBytesPerEntity !== undefined) {
    staticBudget = `${formatBytesLiteral(rss.staticBudgetBytesPerEntity)}/entity × ${entityLabel}`
  }

  return { processRss, perEntity, staticBudget }
}
