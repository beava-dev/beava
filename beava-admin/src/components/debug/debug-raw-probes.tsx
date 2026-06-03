import type { HttpProbeResult } from "@/lib/http-probe"

type DebugRawProbesProps = {
  probes: HttpProbeResult[]
}

export function DebugRawProbes({ probes }: DebugRawProbesProps) {
  return (
    <div className="space-y-2">
      {probes.map((probe) => (
        <details
          key={probe.id}
          className="rounded-lg border border-border bg-muted/20 px-4 py-3"
        >
          <summary className="cursor-pointer text-sm font-medium">
            {probe.label}
            <span className="ml-2 font-normal text-muted-foreground">
              {probe.error
                ? probe.error
                : `${probe.status} · ${probe.durationMs} ms`}
            </span>
          </summary>
          <div className="mt-3 space-y-2 font-mono text-xs text-muted-foreground">
            <p>
              {probe.method} {probe.url}
            </p>
            {probe.contentType ? <p>Content-Type: {probe.contentType}</p> : null}
            {probe.runtime ? <p>X-Runtime: {probe.runtime}</p> : null}
            <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md bg-background p-3 text-foreground">
              {probe.body || "(empty body)"}
            </pre>
          </div>
        </details>
      ))}
    </div>
  )
}
