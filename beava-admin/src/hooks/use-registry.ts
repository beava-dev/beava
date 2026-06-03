import { fetchRegistryOverview } from "@/lib/registry-overview"
import { usePollingResource } from "@/hooks/use-polling-resource"

export function useRegistry() {
  return usePollingResource(fetchRegistryOverview, 5_000)
}
