import { beavaConfig } from "@/lib/config"
import { getJson, getText } from "@/lib/http-client"

export type HealthResponse = {
  status: "ok"
}

export type ReadyResponse = {
  status: "ready"
}

export type RegistryResponse = {
  version: number
  node_count: number
}

export function getHealth() {
  return getJson<HealthResponse>(`${beavaConfig.adminUrl}/health`)
}

export function getReady() {
  return getJson<ReadyResponse>(`${beavaConfig.adminUrl}/ready`)
}

export function getRegistry() {
  return getJson<RegistryResponse>(`${beavaConfig.adminUrl}/registry`)
}

export function getMetricsText() {
  return getText(`${beavaConfig.adminUrl}/metrics`)
}
