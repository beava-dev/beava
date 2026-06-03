#!/usr/bin/env node
/**
 * PID 1 for the admin container: memory-profile sidecar + Caddy.
 */

import { spawn } from "node:child_process"
import { fileURLToPath } from "node:url"

const PROFILE_URL =
  process.env.BEAVA_MEMORY_PROFILE_HEALTH_URL ??
  "http://127.0.0.1:8091/memory-profile"
const PROFILE_SCRIPT = fileURLToPath(
  new URL("../server/memory-profile.mjs", import.meta.url)
)

function waitForProfile(maxAttempts = 50) {
  return new Promise((resolve, reject) => {
    let attempt = 0
    const tick = async () => {
      attempt += 1
      try {
        const response = await fetch(PROFILE_URL, { signal: AbortSignal.timeout(500) })
        if (response.ok) {
          resolve()
          return
        }
      } catch {
        // not ready yet
      }
      if (attempt >= maxAttempts) {
        reject(new Error(`memory-profile did not become ready: ${PROFILE_URL}`))
        return
      }
      setTimeout(tick, 100)
    }
    tick()
  })
}

function forwardSignals(child) {
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => {
      child.kill(signal)
    })
  }
}

const profile = spawn(process.execPath, [PROFILE_SCRIPT], {
  stdio: "inherit",
  env: process.env,
})

profile.on("exit", (code, signal) => {
  console.error(
    `[supervisor] memory-profile exited code=${code ?? "?"} signal=${signal ?? "?"}`
  )
  if (code !== 0 && code !== null) {
    process.exit(code ?? 1)
  }
})

forwardSignals(profile)

try {
  await waitForProfile()
  console.error("[supervisor] memory-profile ready")
} catch (error) {
  console.error("[supervisor]", error)
  process.exit(1)
}

const caddy = spawn(
  "caddy",
  ["run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"],
  { stdio: "inherit", env: process.env }
)

caddy.on("exit", (code, signal) => {
  console.error(
    `[supervisor] caddy exited code=${code ?? "?"} signal=${signal ?? "?"}`
  )
  profile.kill("SIGTERM")
  process.exit(code ?? 0)
})

forwardSignals(caddy)
