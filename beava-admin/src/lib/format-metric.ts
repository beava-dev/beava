export function formatInteger(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(
    value
  )
}

export function formatDecimal(value: number, digits = 2): string {
  return new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 0,
    maximumFractionDigits: digits,
  }).format(value)
}

export function formatBytes(value: number): string {
  if (value < 1024) {
    return `${formatInteger(value)} B`
  }

  const units = ["KiB", "MiB", "GiB", "TiB"] as const
  let size = value / 1024
  let unitIndex = 0

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex += 1
  }

  return `${formatDecimal(size)} ${units[unitIndex]}`
}

export function formatMetricTotal(value: number | undefined): string {
  if (value === undefined) {
    return "—"
  }

  return formatInteger(value)
}

export function formatCounterRate(
  rate: number | undefined,
  total: number | undefined
): string {
  if (rate !== undefined) {
    return formatRatePerSecond(rate)
  }

  if (total !== undefined) {
    return "0 /s"
  }

  return "—"
}

export function formatRatePerSecond(value: number): string {
  if (value === 0) {
    return "0 /s"
  }

  if (value < 0.01) {
    return "<0.01 /s"
  }

  if (value < 10) {
    return `${formatDecimal(value, 2)} /s`
  }

  return `${formatDecimal(value, 1)} /s`
}

export function formatDurationSeconds(seconds: number): string {
  if (seconds < 1) {
    return `${formatDecimal(seconds * 1000, 0)} ms`
  }

  if (seconds < 60) {
    return `${formatDecimal(seconds, 2)} s`
  }

  return `${formatDecimal(seconds / 60, 1)} min`
}
