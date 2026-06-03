import { getHealth, getReady } from "@/lib/admin-api"
import { usePollingResource } from "@/hooks/use-polling-resource"

async function getAdminHealth() {
  const [health, ready] = await Promise.all([getHealth(), getReady()])

  return { health, ready }
}

export function useAdminHealth() {
  return usePollingResource(getAdminHealth, 3_000)
}
