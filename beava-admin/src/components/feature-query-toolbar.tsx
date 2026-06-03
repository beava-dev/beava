import { DASHBOARD_PROFILES } from "@/lib/dashboard-profiles"
import type { DashboardProfileId } from "@/lib/dashboard-query-types"
import type { DashboardQueriesState } from "@/hooks/use-dashboard-queries"
import { Input } from "@/components/ui/input"

const PROFILE_ORDER: DashboardProfileId[] = [
  "registry",
  "bench-medium",
  "bench-small",
]

type FeatureQueryToolbarProps = {
  dashboard: DashboardQueriesState
}

function sourceLabel(source: DashboardQueriesState["source"]): string {
  switch (source) {
    case "env-queries":
      return "env queries (locked)"
    case "env-profile":
      return "env profile (locked)"
    case "registry":
      return "registry auto"
    case "profile":
      return "saved profile"
    default:
      return source
  }
}

export function FeatureQueryToolbar({ dashboard }: FeatureQueryToolbarProps) {
  const {
    profileId,
    setProfileId,
    isConfigLocked,
    isProfileLocked,
    registryAvailable,
    missingKeyedTargets,
    sampleKeys,
    setSampleKey,
    source,
    queries,
  } = dashboard

  return (
    <div className="space-y-4 rounded-lg border border-border bg-muted/30 p-4">
      <div className="flex flex-wrap items-end gap-4">
        <div className="min-w-[12rem] flex-1 space-y-1">
          <label
            htmlFor="dashboard-profile"
            className="text-xs font-medium uppercase tracking-wide text-muted-foreground"
          >
            Query profile
          </label>
          <select
            id="dashboard-profile"
            className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
            value={profileId}
            disabled={isConfigLocked || isProfileLocked}
            onChange={(event) =>
              setProfileId(event.target.value as DashboardProfileId)
            }
          >
            {PROFILE_ORDER.map((id) => {
              const profile = DASHBOARD_PROFILES[id]
              const disabled =
                id === "registry" && !registryAvailable && !isProfileLocked

              return (
                <option key={id} value={id} disabled={disabled}>
                  {profile.label}
                  {disabled ? " (registry unavailable)" : ""}
                </option>
              )
            })}
          </select>
          <p className="text-xs text-muted-foreground">
            {DASHBOARD_PROFILES[profileId].description}
          </p>
        </div>
        <div className="text-xs text-muted-foreground">
          <span className="font-medium text-foreground">Source:</span>{" "}
          {sourceLabel(source)}
          {queries[0]?.features && queries[0].features.length > 0 ? (
            <span className="ml-2">
              (features: {queries[0].features.join(", ")})
            </span>
          ) : null}
          {queries.length > 0 ? (
            <span className="ml-2">
              ({queries.length} {queries.length === 1 ? "query" : "queries"})
            </span>
          ) : null}
        </div>
      </div>

      {isConfigLocked ? (
        <p className="text-xs text-muted-foreground">
          <code className="text-foreground">VITE_BEAVA_DASHBOARD_QUERIES</code>{" "}
          overrides profiles. Remove it to use the selector.
        </p>
      ) : null}

      {profileId === "registry" && !registryAvailable ? (
        <p className="text-xs text-muted-foreground">
          Enable <code className="text-foreground">dev_endpoints</code> on the
          beava server (or run with <code className="text-foreground">--test-mode</code>
          ) so GET /registry is available, or pick Bench (small).
        </p>
      ) : null}

      {missingKeyedTargets.length > 0 ? (
        <div className="space-y-3">
          <p className="text-xs text-muted-foreground">
            Keyed tables need a sample entity key for POST /get:
          </p>
          {missingKeyedTargets.map((target) => (
            <div key={target.table} className="flex flex-wrap items-center gap-2">
              <label
                htmlFor={`sample-key-${target.table}`}
                className="min-w-[6rem] font-mono text-xs text-foreground"
              >
                {target.table}
              </label>
              <Input
                id={`sample-key-${target.table}`}
                className="max-w-xs font-mono text-xs"
                placeholder={target.primaryKeyFields.join(", ")}
                value={sampleKeys[target.table] ?? ""}
                onChange={(event) =>
                  setSampleKey(target.table, event.target.value)
                }
              />
            </div>
          ))}
        </div>
      ) : null}
    </div>
  )
}
