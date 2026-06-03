export type PrometheusSample = {
  name: string
  labels: Record<string, string>
  value: number
}

const METRIC_LINE =
  /^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{[^}]*\})?\s+(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)(?:\s+\d+)?$/

function parseLabels(raw: string): Record<string, string> {
  const labels: Record<string, string> = {}
  const inner = raw.slice(1, -1).trim()
  if (!inner) {
    return labels
  }

  for (const part of inner.split(",")) {
    const eq = part.indexOf("=")
    if (eq === -1) {
      continue
    }
    const key = part.slice(0, eq).trim()
    let value = part.slice(eq + 1).trim()
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1)
    }
    labels[key] = value
  }

  return labels
}

function parseMetricLine(line: string): PrometheusSample | null {
  const match = METRIC_LINE.exec(line.trim())
  if (!match) {
    return null
  }

  return {
    name: match[1],
    labels: match[2] ? parseLabels(match[2]) : {},
    value: Number(match[3]),
  }
}

export function parsePrometheusText(text: string): PrometheusSample[] {
  const samples: PrometheusSample[] = []

  for (const line of text.split("\n")) {
    const sample = parseMetricLine(line)
    if (sample !== null && Number.isFinite(sample.value)) {
      samples.push(sample)
    }
  }

  return samples
}

export function findMetric(
  samples: PrometheusSample[],
  name: string,
  labels: Record<string, string> = {}
): number | undefined {
  const labelEntries = Object.entries(labels)

  for (const sample of samples) {
    if (sample.name !== name) {
      continue
    }

    const matches = labelEntries.every(
      ([key, value]) => sample.labels[key] === value
    )
    if (!matches) {
      continue
    }

    if (labelEntries.length === 0 && Object.keys(sample.labels).length > 0) {
      continue
    }

    return sample.value
  }

  return undefined
}
