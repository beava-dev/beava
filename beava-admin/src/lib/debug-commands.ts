import { beavaConfig } from "@/lib/config"
import {
  benchRegisterMedium,
  benchRegisterSmall,
  benchSampleKey,
} from "@/lib/debug-command-payloads"
import { httpProbe, type HttpProbeResult } from "@/lib/http-probe"

const jsonHeaders = { "content-type": "application/json" }

export type DebugCommandId =
  | "ping"
  | "health-both"
  | "register-medium"
  | "register-small"
  | "push-txn-once"
  | "push-txn-burst"
  | "get-txn-agg"
  | "metrics-smoke"
  | "reset"

export type DebugCommandCategory = "probe" | "registry" | "load" | "smoke" | "danger"

export type DebugCommandDefinition = {
  id: DebugCommandId
  category: DebugCommandCategory
  label: string
  description: string
  destructive?: boolean
  confirmMessage?: string
}

export type DebugCommandRunResult = {
  commandId: DebugCommandId
  label: string
  startedAt: string
  steps: HttpProbeResult[]
}

export const DEBUG_COMMANDS: DebugCommandDefinition[] = [
  {
    id: "ping",
    category: "probe",
    label: "Ping data plane",
    description: "POST /ping — live registry_version on the mio plane.",
  },
  {
    id: "health-both",
    category: "probe",
    label: "Health check (admin + data)",
    description: "GET /health on ports 8090 and 8080.",
  },
  {
    id: "register-medium",
    category: "registry",
    label: "Register bench medium (force)",
    description:
      "POST /register with configs/medium.json (TxnAgg: cnt, sum, avg, min, max).",
  },
  {
    id: "register-small",
    category: "registry",
    label: "Register bench small (force)",
    description: "POST /register with configs/small.json (TxnAgg: cnt only).",
  },
  {
    id: "push-txn-once",
    category: "load",
    label: "Push one Txn event",
    description: `POST /push/Txn for user ${benchSampleKey}.`,
  },
  {
    id: "push-txn-burst",
    category: "load",
    label: "Push 100 Txn events",
    description: "Burst load for metrics / Features (same shape as metrics-smoke.sh).",
  },
  {
    id: "get-txn-agg",
    category: "load",
    label: "GET TxnAgg row",
    description: `POST /get for table TxnAgg, key ${benchSampleKey}.`,
  },
  {
    id: "metrics-smoke",
    category: "smoke",
    label: "Metrics smoke",
    description:
      "Register medium (force), push 100 events, scrape admin /metrics and /registry.",
  },
  {
    id: "reset",
    category: "danger",
    label: "Reset server (test mode)",
    description:
      "POST /reset — clears state. Requires beava --test-mode or BEAVA_TEST_MODE=1.",
    destructive: true,
    confirmMessage:
      "Reset wipes in-memory state and bumps the registry. Only use on a test-mode server. Continue?",
  },
]

const CATEGORY_LABELS: Record<DebugCommandCategory, string> = {
  probe: "Probes",
  registry: "Registry",
  load: "Load",
  smoke: "Smoke",
  danger: "Danger zone",
}

export function debugCommandCategories(): DebugCommandCategory[] {
  return ["probe", "registry", "load", "smoke", "danger"]
}

export function commandsForCategory(
  category: DebugCommandCategory
): DebugCommandDefinition[] {
  return DEBUG_COMMANDS.filter((command) => command.category === category)
}

export function getDebugCommand(id: DebugCommandId): DebugCommandDefinition {
  const command = DEBUG_COMMANDS.find((entry) => entry.id === id)
  if (!command) {
    throw new Error(`unknown debug command: ${id}`)
  }

  return command
}

export function categoryLabel(category: DebugCommandCategory): string {
  return CATEGORY_LABELS[category]
}

function registerBody(nodes: typeof benchRegisterMedium.nodes) {
  return JSON.stringify({ nodes, force: true })
}

function txnPushBody(sequence: number) {
  const amount = Math.round((1 + Math.random() * 499) * 100) / 100
  return JSON.stringify({
    user_id: benchSampleKey,
    amount,
    event_time: 1_000_000 + sequence,
  })
}

async function pushTxnBurst(count: number): Promise<HttpProbeResult> {
  const label = `Push ${count}× Txn`
  const url = `${beavaConfig.dataUrl}/push/Txn`
  const started = performance.now()
  let succeeded = 0
  let failed = 0
  const errors: string[] = []

  for (let index = 0; index < count; index += 1) {
    try {
      const res = await fetch(url, {
        method: "POST",
        headers: jsonHeaders,
        body: txnPushBody(index),
      })

      if (res.ok) {
        succeeded += 1
      } else {
        failed += 1
        if (errors.length < 3) {
          const detail = await res.text().catch(() => "")
          errors.push(`${res.status}: ${detail.slice(0, 120)}`)
        }
      }
    } catch (error) {
      failed += 1
      if (errors.length < 3) {
        errors.push(error instanceof Error ? error.message : String(error))
      }
    }
  }

  const durationMs = Math.round(performance.now() - started)
  const body = JSON.stringify(
    { pushed: count, succeeded, failed, sample_errors: errors },
    null,
    2
  )

  return {
    id: "push-txn-burst-loop",
    label,
    method: "POST",
    url,
    ok: failed === 0,
    status: failed === 0 ? 200 : 207,
    statusText: failed === 0 ? "OK" : "partial",
    durationMs,
    contentType: "application/json",
    runtime: "mio",
    body,
    parsedJson: JSON.parse(body) as unknown,
    error: failed > 0 ? `${failed} pushes failed` : undefined,
  }
}

function filterMetricLines(text: string): string {
  const prefixes = [
    "beava_registry_version ",
    "beava_node_count ",
    "beava_entity_count_resident ",
    "beava_snapshot_last_bytes ",
    "beava_bucket_reclaim_total ",
  ]

  return text
    .split("\n")
    .filter((line) => prefixes.some((prefix) => line.startsWith(prefix)))
    .join("\n")
}

async function runSteps(
  commandId: DebugCommandId,
  steps: HttpProbeResult[]
): Promise<DebugCommandRunResult> {
  return {
    commandId,
    label: getDebugCommand(commandId).label,
    startedAt: new Date().toISOString(),
    steps,
  }
}

export async function runDebugCommand(
  commandId: DebugCommandId
): Promise<DebugCommandRunResult> {
  switch (commandId) {
    case "ping":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-ping",
          label: "POST /ping",
          method: "POST",
          url: `${beavaConfig.dataUrl}/ping`,
          body: "{}",
          headers: jsonHeaders,
        }),
      ])

    case "health-both":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-admin-health",
          label: "Admin GET /health",
          url: `${beavaConfig.adminUrl}/health`,
        }),
        await httpProbe({
          id: "cmd-data-health",
          label: "Data GET /health",
          url: `${beavaConfig.dataUrl}/health`,
        }),
      ])

    case "register-medium":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-register-medium",
          label: "POST /register (medium, force)",
          method: "POST",
          url: `${beavaConfig.dataUrl}/register`,
          body: registerBody(benchRegisterMedium.nodes),
          headers: jsonHeaders,
        }),
      ])

    case "register-small":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-register-small",
          label: "POST /register (small, force)",
          method: "POST",
          url: `${beavaConfig.dataUrl}/register`,
          body: registerBody(benchRegisterSmall.nodes),
          headers: jsonHeaders,
        }),
      ])

    case "push-txn-once":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-push-once",
          label: "POST /push/Txn",
          method: "POST",
          url: `${beavaConfig.dataUrl}/push/Txn`,
          body: txnPushBody(0),
          headers: jsonHeaders,
        }),
      ])

    case "push-txn-burst":
      return runSteps(commandId, [await pushTxnBurst(100)])

    case "get-txn-agg":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-get-txn-agg",
          label: "POST /get TxnAgg",
          method: "POST",
          url: `${beavaConfig.dataUrl}/get`,
          body: JSON.stringify({
            table: "TxnAgg",
            key: benchSampleKey,
          }),
          headers: jsonHeaders,
        }),
      ])

    case "metrics-smoke": {
      const steps: HttpProbeResult[] = [
        await httpProbe({
          id: "cmd-smoke-register",
          label: "POST /register (medium, force)",
          method: "POST",
          url: `${beavaConfig.dataUrl}/register`,
          body: registerBody(benchRegisterMedium.nodes),
          headers: jsonHeaders,
        }),
      ]

      const metricsBefore = await httpProbe({
        id: "cmd-smoke-metrics-before",
        label: "Admin GET /metrics (before)",
        url: `${beavaConfig.adminUrl}/metrics`,
      })
      steps.push({
        ...metricsBefore,
        body: filterMetricLines(metricsBefore.body) || metricsBefore.body,
      })

      steps.push(await pushTxnBurst(100))

      const metricsAfter = await httpProbe({
        id: "cmd-smoke-metrics-after",
        label: "Admin GET /metrics (after)",
        url: `${beavaConfig.adminUrl}/metrics`,
      })
      steps.push({
        ...metricsAfter,
        body: filterMetricLines(metricsAfter.body) || metricsAfter.body,
      })

      steps.push(
        await httpProbe({
          id: "cmd-smoke-admin-registry",
          label: "Admin GET /registry",
          url: `${beavaConfig.adminUrl}/registry`,
        })
      )

      return runSteps(commandId, steps)
    }

    case "reset":
      return runSteps(commandId, [
        await httpProbe({
          id: "cmd-reset",
          label: "POST /reset",
          method: "POST",
          url: `${beavaConfig.dataUrl}/reset`,
          body: "{}",
          headers: jsonHeaders,
        }),
      ])

    default: {
      const exhaustive: never = commandId
      throw new Error(`unhandled command: ${exhaustive}`)
    }
  }
}
