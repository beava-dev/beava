export type { DashboardGetQuery } from "@/lib/dashboard-query-types"

export const beavaConfig = {
  adminUrl: import.meta.env.VITE_BEAVA_ADMIN_URL ?? "/api/admin",
  dataUrl: import.meta.env.VITE_BEAVA_DATA_URL ?? "/api/data",
  grafanaUrl: import.meta.env.VITE_GRAFANA_URL ?? "",
  prometheusUrl: import.meta.env.VITE_PROMETHEUS_URL ?? "",
}
