import { useCallback } from "react"

import { captureDebugSnapshot } from "@/lib/debug-probes"
import { usePollingResource } from "@/hooks/use-polling-resource"

const DEBUG_POLL_MS = 10_000

export function useDebugSnapshot() {
  const queryFn = useCallback(captureDebugSnapshot, [])

  return usePollingResource(queryFn, DEBUG_POLL_MS)
}
