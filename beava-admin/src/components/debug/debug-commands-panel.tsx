import { CollapsibleSection } from "@/components/collapsible-section"
import { DebugProbeTable } from "@/components/debug/debug-probe-table"
import { DebugRawProbes } from "@/components/debug/debug-raw-probes"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { useDebugCommand } from "@/hooks/use-debug-command"
import {
  categoryLabel,
  commandsForCategory,
  debugCommandCategories,
  type DebugCommandDefinition,
  type DebugCommandId,
} from "@/lib/debug-commands"

type DebugCommandsPanelProps = {
  onCommandComplete?: () => void
}

function CommandRow({
  command,
  runningId,
  onRun,
}: {
  command: DebugCommandDefinition
  runningId: DebugCommandId | null
  onRun: (id: DebugCommandId) => void
}) {
  const isRunning = runningId === command.id
  const isBlocked = runningId !== null && !isRunning

  return (
    <div className="flex flex-wrap items-start justify-between gap-3 border-b border-border py-3 last:border-0">
      <div className="min-w-0 flex-1 space-y-1">
        <p className="text-sm font-medium text-foreground">{command.label}</p>
        <p className="text-xs text-muted-foreground">{command.description}</p>
      </div>
      <Button
        type="button"
        size="sm"
        variant={command.destructive ? "destructive" : "outline"}
        disabled={isBlocked}
        onClick={() => onRun(command.id)}
      >
        {isRunning ? "Running…" : "Run"}
      </Button>
    </div>
  )
}

export function DebugCommandsPanel({
  onCommandComplete,
}: DebugCommandsPanelProps) {
  const { runningId, lastResult, error, run, clear, isRunning } =
    useDebugCommand()

  async function handleRun(commandId: DebugCommandId) {
    const result = await run(commandId)
    if (result) {
      onCommandComplete?.()
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Debug commands</CardTitle>
        <CardDescription>
          Run the same HTTP calls as{" "}
          <code className="text-xs">scripts/metrics-smoke.sh</code> and{" "}
          <code className="text-xs">scripts/load-bench-medium.sh</code> through
          the Vite proxy. Responses appear below; refresh probes to update the
          snapshot.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        {debugCommandCategories().map((category) => (
          <div key={category}>
            <h3 className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {categoryLabel(category)}
            </h3>
            <div className="rounded-lg border border-border px-3">
              {commandsForCategory(category).map((command) => (
                <CommandRow
                  key={command.id}
                  command={command}
                  runningId={runningId}
                  onRun={handleRun}
                />
              ))}
            </div>
          </div>
        ))}

        {error ? (
          <p className="text-sm text-destructive">
            Command failed: {error.message}
          </p>
        ) : null}

        {lastResult ? (
          <div className="space-y-3 rounded-lg border border-border bg-muted/20 p-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <p className="text-sm font-medium">{lastResult.label}</p>
                <p className="text-xs text-muted-foreground">
                  Finished {new Date(lastResult.startedAt).toLocaleString()} ·{" "}
                  {lastResult.steps.length}{" "}
                  {lastResult.steps.length === 1 ? "step" : "steps"}
                </p>
              </div>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                disabled={isRunning}
                onClick={clear}
              >
                Clear
              </Button>
            </div>
            <DebugProbeTable probes={lastResult.steps} />
            <CollapsibleSection
              variant="panel"
              title="Step bodies"
              description="Full HTTP response bodies for each command step."
            >
              <DebugRawProbes probes={lastResult.steps} />
            </CollapsibleSection>
          </div>
        ) : null}
      </CardContent>
    </Card>
  )
}
