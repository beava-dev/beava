import type { MetricRow } from "@/lib/beava-metrics"

type DebugMetricsTableProps = {
  rows: MetricRow[]
}

export function DebugMetricsTable({ rows }: DebugMetricsTableProps) {
  if (rows.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No metrics parsed. Check admin /metrics probe.
      </p>
    )
  }

  return (
    <div className="overflow-x-auto rounded-lg border border-border">
      <table className="w-full text-left text-sm">
        <thead className="border-b border-border bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">Metric</th>
            <th className="px-3 py-2 font-medium">Labels</th>
            <th className="px-3 py-2 font-medium">Value</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr
              key={`${row.name}-${row.labels}-${index}`}
              className="border-b border-border last:border-0"
            >
              <td className="px-3 py-2 font-mono text-xs">{row.name}</td>
              <td className="px-3 py-2 font-mono text-xs text-muted-foreground">
                {row.labels}
              </td>
              <td className="px-3 py-2 tabular-nums">{row.value}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
