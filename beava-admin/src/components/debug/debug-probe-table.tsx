import { Badge } from "@/components/ui/badge"
import type { HttpProbeResult } from "@/lib/http-probe"

function statusVariant(probe: HttpProbeResult) {
  if (probe.error) {
    return "destructive" as const
  }

  if (probe.ok) {
    return "default" as const
  }

  return "secondary" as const
}

type DebugProbeTableProps = {
  probes: HttpProbeResult[]
}

export function DebugProbeTable({ probes }: DebugProbeTableProps) {
  return (
    <div className="overflow-x-auto rounded-lg border border-border">
      <table className="w-full text-left text-sm">
        <thead className="border-b border-border bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">Endpoint</th>
            <th className="px-3 py-2 font-medium">Status</th>
            <th className="px-3 py-2 font-medium">Latency</th>
            <th className="px-3 py-2 font-medium">Runtime</th>
          </tr>
        </thead>
        <tbody>
          {probes.map((probe) => (
            <tr key={probe.id} className="border-b border-border last:border-0">
              <td className="px-3 py-2">
                <div className="font-medium">{probe.label}</div>
                <div className="font-mono text-xs text-muted-foreground">
                  {probe.method} {probe.url}
                </div>
              </td>
              <td className="px-3 py-2">
                <Badge variant={statusVariant(probe)}>
                  {probe.error
                    ? "error"
                    : probe.status > 0
                      ? `${probe.status}`
                      : "—"}
                </Badge>
              </td>
              <td className="px-3 py-2 tabular-nums">{probe.durationMs} ms</td>
              <td className="px-3 py-2 font-mono text-xs">
                {probe.runtime ?? "—"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
