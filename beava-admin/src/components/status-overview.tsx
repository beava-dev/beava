import { MetricRow } from "@/components/metric-row"
import { ProbeStatusBadge } from "@/components/status-badge"
import { StatusCard } from "@/components/status-card"
import { useAdminHealth } from "@/hooks/use-admin"
import { useRegistry } from "@/hooks/use-registry"

export function StatusOverview() {
  const registry = useRegistry()
  const adminHealth = useAdminHealth()

  return (
    <div className="grid gap-4 md:grid-cols-2">
      <StatusCard
        title="Registry"
        description="Admin /registry with data-plane /ping and dev /registry fallback"
        resource={registry}
        errorMessage="Unable to reach the Beava server."
        renderContent={(data) => (
          <dl>
            <MetricRow label="Version" value={data.version} />
            <MetricRow label="Nodes" value={data.node_count} />
          </dl>
        )}
      />

      <StatusCard
        title="Admin health"
        description="Liveness and readiness probes on the admin sidecar"
        resource={adminHealth}
        errorMessage="Unable to fetch the Beava admin health endpoints."
        renderContent={(data) => (
          <dl>
            <MetricRow
              label="Health"
              value={<ProbeStatusBadge status={data.health.status} />}
            />
            <MetricRow
              label="Ready"
              value={<ProbeStatusBadge status={data.ready.status} />}
            />
          </dl>
        )}
      />
    </div>
  )
}
