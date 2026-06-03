import { MetricRow } from "@/components/metric-row"
import { getEnvDashboardQueries } from "@/lib/dashboard-config"
import { beavaConfig } from "@/lib/config"

export function DebugTargets() {
  return (
    <dl>
      <MetricRow
        label="Admin URL"
        value={<code className="text-xs break-all">{beavaConfig.adminUrl}</code>}
      />
      <MetricRow
        label="Data URL"
        value={<code className="text-xs break-all">{beavaConfig.dataUrl}</code>}
      />
      <MetricRow
        label="Dashboard queries"
        value={
          <code className="text-xs break-all">
            {JSON.stringify(
              getEnvDashboardQueries() ?? "(resolved in Features page)"
            )}
          </code>
        }
      />
    </dl>
  )
}
