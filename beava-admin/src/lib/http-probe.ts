export type HttpProbeRequest = {
  id: string
  label: string
  method?: "GET" | "POST"
  url: string
  body?: string
  headers?: Record<string, string>
}

export type HttpProbeResult = {
  id: string
  label: string
  method: string
  url: string
  ok: boolean
  status: number
  statusText: string
  durationMs: number
  contentType: string | null
  runtime: string | null
  body: string
  parsedJson: unknown | undefined
  error: string | undefined
}

function tryParseJson(text: string): unknown | undefined {
  const trimmed = text.trim()
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) {
    return undefined
  }

  try {
    return JSON.parse(trimmed) as unknown
  } catch {
    return undefined
  }
}

export async function httpProbe(
  request: HttpProbeRequest
): Promise<HttpProbeResult> {
  const method = request.method ?? "GET"
  const started = performance.now()

  try {
    const res = await fetch(request.url, {
      method,
      headers: request.headers,
      body: request.body,
    })

    const body = await res.text()
    const durationMs = Math.round(performance.now() - started)

    return {
      id: request.id,
      label: request.label,
      method,
      url: request.url,
      ok: res.ok,
      status: res.status,
      statusText: res.statusText,
      durationMs,
      contentType: res.headers.get("content-type"),
      runtime: res.headers.get("x-runtime"),
      body,
      parsedJson: tryParseJson(body),
      error: undefined,
    }
  } catch (error) {
    const durationMs = Math.round(performance.now() - started)

    return {
      id: request.id,
      label: request.label,
      method,
      url: request.url,
      ok: false,
      status: 0,
      statusText: "network_error",
      durationMs,
      contentType: null,
      runtime: null,
      body: "",
      parsedJson: undefined,
      error: error instanceof Error ? error.message : String(error),
    }
  }
}
