import { useCallback, useState } from "react"

import {
  getDebugCommand,
  runDebugCommand,
  type DebugCommandId,
  type DebugCommandRunResult,
} from "@/lib/debug-commands"

function toError(error: unknown) {
  return error instanceof Error ? error : new Error(String(error))
}

export function useDebugCommand() {
  const [runningId, setRunningId] = useState<DebugCommandId | null>(null)
  const [lastResult, setLastResult] = useState<DebugCommandRunResult | null>(
    null
  )
  const [error, setError] = useState<Error | null>(null)

  const run = useCallback(async (commandId: DebugCommandId) => {
    const command = getDebugCommand(commandId)

    if (command.destructive && command.confirmMessage) {
      if (!window.confirm(command.confirmMessage)) {
        return null
      }
    }

    setRunningId(commandId)
    setError(null)

    try {
      const result = await runDebugCommand(commandId)
      setLastResult(result)
      return result
    } catch (nextError) {
      const wrapped = toError(nextError)
      setError(wrapped)
      return null
    } finally {
      setRunningId(null)
    }
  }, [])

  const clear = useCallback(() => {
    setLastResult(null)
    setError(null)
  }, [])

  return {
    runningId,
    lastResult,
    error,
    run,
    clear,
    isRunning: runningId !== null,
  }
}
