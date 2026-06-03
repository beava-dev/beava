import { MetricsOverview } from "@/components/metrics-overview"
import { PageShell } from "@/components/page-shell"

export default function MetricsPage() {
  return (
    <PageShell
      title="Metrics"
      description="Admin /metrics plus an RSS memory profiler (admin-side only). Counter rates use poll deltas."
    >
      <MetricsOverview />
    </PageShell>
  )
}
