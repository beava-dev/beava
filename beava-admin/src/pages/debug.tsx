import { DebugOverview } from "@/components/debug/debug-overview"
import { PageShell } from "@/components/page-shell"
import { useDebugSnapshot } from "@/hooks/use-debug-snapshot"

export default function DebugPage() {
  const debug = useDebugSnapshot()

  return (
    <PageShell
      title="Debug"
      description="Run debug HTTP commands, probe admin and data-plane endpoints, and inspect responses."
    >
      <DebugOverview resource={debug} />
    </PageShell>
  )
}
