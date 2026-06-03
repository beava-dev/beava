import { useCallback, useEffect, useState } from "react"

type PollingStatus = "loading" | "success" | "error"

export type PollingResource<T> = {
  data: T | undefined
  error: Error | undefined
  status: PollingStatus
  isLoading: boolean
  isSuccess: boolean
  isError: boolean
  refetch: () => Promise<void>
}

function toError(error: unknown) {
  return error instanceof Error ? error : new Error(String(error))
}

export function usePollingResource<T>(
  queryFn: () => Promise<T>,
  intervalMs: number
): PollingResource<T> {
  const [data, setData] = useState<T>()
  const [error, setError] = useState<Error>()
  const [status, setStatus] = useState<PollingStatus>("loading")

  const refetch = useCallback(async () => {
    try {
      const nextData = await queryFn()

      setData(nextData)
      setError(undefined)
      setStatus("success")
    } catch (nextError) {
      setError(toError(nextError))
      setStatus("error")
    }
  }, [queryFn])

  useEffect(() => {
    let cancelled = false

    async function run(showLoading: boolean) {
      if (showLoading && !cancelled) {
        setStatus("loading")
      }

      try {
        const nextData = await queryFn()

        if (!cancelled) {
          setData(nextData)
          setError(undefined)
          setStatus("success")
        }
      } catch (nextError) {
        if (!cancelled) {
          setError(toError(nextError))
          setStatus("error")
        }
      }
    }

    void run(true)
    const intervalId = window.setInterval(() => void run(false), intervalMs)

    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [queryFn, intervalMs])

  return {
    data,
    error,
    status,
    isLoading: status === "loading",
    isSuccess: status === "success",
    isError: status === "error",
    refetch,
  }
}
