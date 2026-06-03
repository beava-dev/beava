import { FeatureDataOverview } from "@/components/feature-data-overview"
import { PageShell } from "@/components/page-shell"

export default function FeaturesPage() {
  return (
    <PageShell
      title="Features"
      description="Live feature rows from data-plane POST /get."
    >
      <FeatureDataOverview />
    </PageShell>
  )
}
