import { MetricRow } from "@/components/metric-row"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import type { DebugSnapshot } from "@/lib/debug-probes"

type DebugRegistryPanelProps = {
  snapshot: DebugSnapshot
}

function NameList({ names }: { names: string[] }) {
  if (names.length === 0) {
    return <span className="text-muted-foreground">(none)</span>
  }

  return (
    <ul className="space-y-1 font-mono text-xs">
      {names.map((name) => (
        <li key={name}>{name}</li>
      ))}
    </ul>
  )
}

export function DebugRegistryPanel({ snapshot }: DebugRegistryPanelProps) {
  const { adminRegistry, dataRegistry, dataPing, registryMismatch } = snapshot

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle>Admin registry</CardTitle>
        </CardHeader>
        <CardContent>
          {adminRegistry ? (
            <dl>
              <MetricRow label="Version" value={adminRegistry.version} />
              <MetricRow label="Nodes" value={adminRegistry.node_count} />
            </dl>
          ) : (
            <p className="text-sm text-muted-foreground">
              Admin /registry unavailable or unparsable.
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            Data registry
            {dataRegistry?.devOnly ? (
              <Badge variant="outline">dev</Badge>
            ) : null}
          </CardTitle>
        </CardHeader>
        <CardContent>
          {dataRegistry ? (
            <div className="space-y-4">
              <dl>
                <MetricRow label="Version" value={dataRegistry.version} />
                <MetricRow
                  label="Events"
                  value={dataRegistry.events.length}
                />
                <MetricRow
                  label="Tables"
                  value={dataRegistry.tables.length}
                />
                <MetricRow
                  label="Derivations"
                  value={dataRegistry.derivations.length}
                />
              </dl>
              <div>
                <p className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  Events
                </p>
                <NameList names={dataRegistry.events} />
              </div>
              <div>
                <p className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  Tables / derivations
                </p>
                <NameList
                  names={[...dataRegistry.tables, ...dataRegistry.derivations]}
                />
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              Data-plane GET /registry returned 404 or is disabled. Enable{" "}
              <code className="text-xs">dev_endpoints</code> on the server (e.g.
              test mode) for the full registry dump.
            </p>
          )}
        </CardContent>
      </Card>

      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>Cross-checks</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {dataPing ? (
            <MetricRow
              label="POST /ping registry_version"
              value={dataPing.registry_version}
            />
          ) : (
            <p className="text-sm text-muted-foreground">POST /ping failed.</p>
          )}
          {registryMismatch === true ? (
            <p className="text-sm text-destructive">
              Admin registry version ({adminRegistry?.version}) differs from
              data registry version ({dataRegistry?.version}). Admin snapshot may
              lag behind the data plane after register.
            </p>
          ) : registryMismatch === false ? (
            <p className="text-sm text-muted-foreground">
              Admin and data registry versions match.
            </p>
          ) : null}
        </CardContent>
      </Card>
    </div>
  )
}
