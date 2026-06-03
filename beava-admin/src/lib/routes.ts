export const routes = {
  overview: "/",
  metrics: "/metrics",
  features: "/features",
  debug: "/debug",
} as const

export type AppRoute = (typeof routes)[keyof typeof routes]

const routeTitles: Record<AppRoute, string> = {
  [routes.overview]: "Overview",
  [routes.metrics]: "Metrics",
  [routes.features]: "Features",
  [routes.debug]: "Debug",
}

export function titleForPath(pathname: string): string {
  if (pathname in routeTitles) {
    return routeTitles[pathname as AppRoute]
  }

  return "Beava Admin"
}
