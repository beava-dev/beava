import { ObservabilityLinks } from "@/components/observability-links"
import { PageShell } from "@/components/page-shell"
import { StatusOverview } from "@/components/status-overview"

export default function OverviewPage() {
  return (
    <PageShell
      title="Overview"
      description="Admin sidecar status: registry snapshot and health probes."
      headerExtra={<ObservabilityLinks />}
    >
      <StatusOverview />
    </PageShell>
  )
}
