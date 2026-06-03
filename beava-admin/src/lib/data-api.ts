import { beavaConfig } from "@/lib/config"
import { getJson, postJson } from "@/lib/http-client"

export type GetRowRequest = {
  table: string
  key: string
  features?: string[]
}

export type FeatureRow = Record<string, unknown>

export type PingResponse = {
  pong: boolean
  registry_version: number
}

export type DataPlaneStatusResponse = {
  status: string
}

export type RegistryDump = {
  version: number
  events: Record<string, unknown>
  tables: Record<string, unknown>
  derivations: Record<string, unknown>
  _dev_only: boolean
}

export function getFeatureRow(request: GetRowRequest) {
  return postJson<FeatureRow, GetRowRequest>(
    `${beavaConfig.dataUrl}/get`,
    request
  )
}

export function postPing() {
  return postJson<PingResponse, Record<string, never>>(
    `${beavaConfig.dataUrl}/ping`,
    {}
  )
}

export function getDataHealth() {
  return getJson<DataPlaneStatusResponse>(`${beavaConfig.dataUrl}/health`)
}

export function getDataReady() {
  return getJson<DataPlaneStatusResponse>(`${beavaConfig.dataUrl}/ready`)
}

export function getDataRegistryDump() {
  return getJson<RegistryDump>(`${beavaConfig.dataUrl}/registry`)
}
