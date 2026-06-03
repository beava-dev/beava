export async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url)

  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`)
  }

  return res.json() as Promise<T>
}

export async function getText(url: string): Promise<string> {
  const res = await fetch(url)

  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`)
  }

  return res.text()
}

export async function postJson<TResponse, TBody extends object>(
  url: string,
  body: TBody
): Promise<TResponse> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  })

  if (!res.ok) {
    const detail = await res.text().catch(() => "")
    throw new Error(
      detail
        ? `${res.status} ${res.statusText}: ${detail}`
        : `${res.status} ${res.statusText}`
    )
  }

  return res.json() as Promise<TResponse>
}
