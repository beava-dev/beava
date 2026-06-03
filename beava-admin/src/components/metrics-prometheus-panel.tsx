import type { MetricRow as PrometheusRow } from "@/lib/beava-metrics"

type MetricsPrometheusPanelProps = {
  rows: PrometheusRow[]
}

export function MetricsPrometheusPanel({ rows }: MetricsPrometheusPanelProps) {
  if (rows.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No <code className="text-xs">beava_*</code> samples in the admin scrape.
      </p>
    )
  }

  return (
    <div className="overflow-x-auto rounded-lg border border-border">
      <table className="w-full text-left text-xs">
        <thead className="bg-muted/50 text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">Metric</th>
            <th className="px-3 py-2 font-medium">Labels</th>
            <th className="px-3 py-2 font-medium text-right">Value</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={`${row.name}:${row.labels}`} className="border-t border-border">
              <td className="px-3 py-2 font-mono text-foreground">{row.name}</td>
              <td className="px-3 py-2 font-mono text-muted-foreground">
                {row.labels}
              </td>
              <td className="px-3 py-2 text-right tabular-nums text-foreground">
                {row.value}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
