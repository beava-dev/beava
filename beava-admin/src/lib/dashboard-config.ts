import {
  DEFAULT_PROFILE_ID,
  DASHBOARD_PROFILES,
  type DashboardProfile,
} from "@/lib/dashboard-profiles"
import type {
  DashboardGetQuery,
  DashboardProfileId,
} from "@/lib/dashboard-query-types"

const PROFILE_STORAGE_KEY = "beava-admin:dashboard-profile"
const SAMPLE_KEYS_STORAGE_KEY = "beava-admin:dashboard-sample-keys"

export function isDashboardProfileId(value: string): value is DashboardProfileId {
  return value in DASHBOARD_PROFILES
}

export function readStoredProfileId(): DashboardProfileId | null {
  const raw = localStorage.getItem(PROFILE_STORAGE_KEY)
  if (!raw) {
    return null
  }

  if (raw === "website") {
    return "bench-medium"
  }

  if (!isDashboardProfileId(raw)) {
    return null
  }

  return raw
}

export function writeStoredProfileId(id: DashboardProfileId) {
  localStorage.setItem(PROFILE_STORAGE_KEY, id)
}

export function readStoredSampleKeys(): Record<string, string> {
  const raw = localStorage.getItem(SAMPLE_KEYS_STORAGE_KEY)
  if (!raw) {
    return {}
  }

  try {
    const parsed = JSON.parse(raw) as unknown
    if (typeof parsed !== "object" || parsed === null) {
      return {}
    }

    return Object.fromEntries(
      Object.entries(parsed).flatMap(([table, key]) =>
        typeof key === "string" && key.length > 0 ? [[table, key]] : []
      )
    )
  } catch {
    return {}
  }
}

export function writeStoredSampleKeys(keys: Record<string, string>) {
  localStorage.setItem(SAMPLE_KEYS_STORAGE_KEY, JSON.stringify(keys))
}

export function getEnvDashboardQueries(): DashboardGetQuery[] | null {
  const raw = import.meta.env.VITE_BEAVA_DASHBOARD_QUERIES
  if (!raw) {
    return null
  }

  try {
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) {
      return null
    }

    return parsed.flatMap((entry) => {
      if (
        typeof entry !== "object" ||
        entry === null ||
        !("table" in entry) ||
        !("key" in entry)
      ) {
        return []
      }

      const table = String((entry as DashboardGetQuery).table)
      const key = String((entry as DashboardGetQuery).key)
      const label =
        "label" in entry && (entry as DashboardGetQuery).label !== undefined
          ? String((entry as DashboardGetQuery).label)
          : table

      return [{ table, key, label }]
    })
  } catch {
    return null
  }
}

export function getEnvDashboardProfile(): DashboardProfileId | null {
  const raw = import.meta.env.VITE_BEAVA_DASHBOARD_PROFILE
  if (!raw) {
    return null
  }

  if (raw === "website") {
    return "bench-medium"
  }

  if (!isDashboardProfileId(raw)) {
    return null
  }

  return raw
}

export function getEnvSampleKeys(): Record<string, string> {
  const raw = import.meta.env.VITE_BEAVA_DASHBOARD_SAMPLE_KEYS
  if (!raw) {
    return {}
  }

  try {
    const parsed = JSON.parse(raw) as unknown
    if (typeof parsed !== "object" || parsed === null) {
      return {}
    }

    return Object.fromEntries(
      Object.entries(parsed).flatMap(([table, key]) =>
        typeof key === "string" ? [[table, String(key)]] : []
      )
    )
  } catch {
    return {}
  }
}

export function isDashboardConfigLocked(): boolean {
  return getEnvDashboardQueries() !== null
}

export function getProfile(id: DashboardProfileId): DashboardProfile {
  return DASHBOARD_PROFILES[id]
}

export function resolveInitialProfileId(): DashboardProfileId {
  return getEnvDashboardProfile() ?? readStoredProfileId() ?? DEFAULT_PROFILE_ID
}

export function mergeSampleKeys(
  ...layers: Array<Record<string, string> | undefined>
): Record<string, string> {
  return Object.assign({}, ...layers)
}

export { PROFILE_STORAGE_KEY, SAMPLE_KEYS_STORAGE_KEY }
