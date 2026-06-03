import { useCallback, useEffect, useMemo, useState } from "react"

import {
  getEnvDashboardQueries,
  getEnvDashboardProfile,
  getEnvSampleKeys,
  getProfile,
  isDashboardConfigLocked,
  mergeSampleKeys,
  readStoredProfileId,
  readStoredSampleKeys,
  resolveInitialProfileId,
  writeStoredProfileId,
  writeStoredSampleKeys,
} from "@/lib/dashboard-config"
import type { DashboardProfileId } from "@/lib/dashboard-query-types"
import type { DashboardGetQuery, QueryableTarget } from "@/lib/dashboard-query-types"
import { planQueriesFromRegistry } from "@/lib/registry-query-planner"
import { getDataRegistryDump, type RegistryDump } from "@/lib/data-api"

export type DashboardQuerySource =
  | "env-queries"
  | "env-profile"
  | "profile"
  | "registry"

export type DashboardQueriesState = {
  queries: DashboardGetQuery[]
  source: DashboardQuerySource
  profileId: DashboardProfileId
  /** Profile driving queries (differs from profileId when registry is selected but unavailable). */
  activeProfileId: DashboardProfileId
  expectedFields: string[]
  setProfileId: (id: DashboardProfileId) => void
  isConfigLocked: boolean
  isProfileLocked: boolean
  registryDump: RegistryDump | undefined
  registryAvailable: boolean
  missingKeyedTargets: QueryableTarget[]
  sampleKeys: Record<string, string>
  setSampleKey: (table: string, key: string) => void
  refreshRegistry: () => Promise<void>
}

const REGISTRY_POLL_MS = 30_000

export function useDashboardQueries(): DashboardQueriesState {
  const envQueries = useMemo(() => getEnvDashboardQueries(), [])
  const envProfile = useMemo(() => getEnvDashboardProfile(), [])
  const envSampleKeys = useMemo(() => getEnvSampleKeys(), [])
  const isConfigLocked = useMemo(() => isDashboardConfigLocked(), [])
  const isProfileLocked = envProfile !== null

  const [profileId, setProfileIdState] = useState<DashboardProfileId>(
    resolveInitialProfileId
  )
  const [sampleKeys, setSampleKeysState] = useState<Record<string, string>>(
    () =>
      mergeSampleKeys(
        getProfile(resolveInitialProfileId()).sampleKeys,
        readStoredSampleKeys(),
        getEnvSampleKeys()
      )
  )
  const [registryDump, setRegistryDump] = useState<RegistryDump | undefined>()

  const refreshRegistry = useCallback(async () => {
    try {
      const dump = await getDataRegistryDump()
      setRegistryDump(dump)
    } catch {
      setRegistryDump(undefined)
    }
  }, [])

  useEffect(() => {
    void refreshRegistry()
    const timer = window.setInterval(() => {
      void refreshRegistry()
    }, REGISTRY_POLL_MS)

    return () => window.clearInterval(timer)
  }, [refreshRegistry])

  const setProfileId = useCallback(
    (id: DashboardProfileId) => {
      if (isProfileLocked) {
        return
      }

      setProfileIdState(id)
      writeStoredProfileId(id)

      const profileKeys = getProfile(id).sampleKeys
      if (profileKeys) {
        setSampleKeysState((prev) => {
          const next = mergeSampleKeys(profileKeys, prev, envSampleKeys)
          writeStoredSampleKeys(next)
          return next
        })
      }
    },
    [envSampleKeys, isProfileLocked]
  )

  const setSampleKey = useCallback(
    (table: string, key: string) => {
      setSampleKeysState((prev) => {
        const next = { ...prev, [table]: key }
        writeStoredSampleKeys(next)
        return next
      })
    },
    []
  )

  const resolved = useMemo(() => {
    if (envQueries && envQueries.length > 0) {
      return {
        activeProfileId: envProfile ?? profileId,
        queries: envQueries,
        expectedFields: [] as string[],
        source: "env-queries" as const,
        missingKeyedTargets: [] as QueryableTarget[],
      }
    }

    const activeProfile = envProfile ?? profileId

    if (activeProfile === "bench-small" || activeProfile === "bench-medium") {
      const profile = getProfile(activeProfile)
      return {
        activeProfileId: activeProfile,
        queries: profile.queries,
        expectedFields: profile.expectedFields ?? [],
        source: (envProfile ? "env-profile" : "profile") as DashboardQuerySource,
        missingKeyedTargets: [] as QueryableTarget[],
      }
    }

    if (registryDump) {
      const keys = mergeSampleKeys(
        getProfile("bench-small").sampleKeys,
        sampleKeys,
        envSampleKeys
      )
      const planned = planQueriesFromRegistry(registryDump, keys)

      if (planned.queries.length > 0) {
        return {
          activeProfileId: "registry" as const,
          queries: planned.queries,
          expectedFields: [],
          source: "registry" as const,
          missingKeyedTargets: planned.missingKeys,
        }
      }
    }

    return {
      activeProfileId: "registry" as const,
      queries: [] as DashboardGetQuery[],
      expectedFields: [] as string[],
      source: "registry" as const,
      missingKeyedTargets: [] as QueryableTarget[],
    }
  }, [envQueries, envProfile, envSampleKeys, profileId, registryDump, sampleKeys])

  return {
    queries: resolved.queries,
    source: resolved.source,
    profileId: envProfile ?? profileId,
    activeProfileId: resolved.activeProfileId,
    expectedFields: resolved.expectedFields,
    setProfileId,
    isConfigLocked,
    isProfileLocked,
    registryDump,
    registryAvailable: registryDump !== undefined,
    missingKeyedTargets: resolved.missingKeyedTargets,
    sampleKeys,
    setSampleKey,
    refreshRegistry,
  }
}
