import { getRegistry } from "@/lib/admin-api"
import { getDataRegistryDump, postPing } from "@/lib/data-api"

export type RegistryOverview = {
  version: number
  node_count: number
}

export async function fetchRegistryOverview(): Promise<RegistryOverview> {
  const [admin, ping] = await Promise.all([getRegistry(), postPing()])

  let version = admin.version
  let nodeCount = admin.node_count

  if (ping.registry_version > 0 && version === 0) {
    version = ping.registry_version
  }

  if (nodeCount === 0) {
    try {
      const dump = await getDataRegistryDump()
      nodeCount =
        Object.keys(dump.events).length +
        Object.keys(dump.tables).length +
        Object.keys(dump.derivations).length
    } catch {
      // GET /registry on data plane requires dev_endpoints
    }
  }

  return { version, node_count: nodeCount }
}
