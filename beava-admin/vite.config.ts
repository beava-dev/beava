import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig, loadEnv } from "vite"

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "")
  const adminTarget =
    env.VITE_BEAVA_ADMIN_PROXY_TARGET ??
    env.VITE_BEAVA_ADMIN_URL ??
    "http://127.0.0.1:8090"
  const dataTarget =
    env.VITE_BEAVA_DATA_PROXY_TARGET ??
    env.VITE_BEAVA_DATA_URL ??
    "http://127.0.0.1:8080"
  const memoryProfileTarget =
    env.VITE_BEAVA_MEMORY_PROFILE_TARGET ?? "http://127.0.0.1:8091"

  return {
    plugins: [react(), tailwindcss()],
    server: {
      proxy: {
        "/api/admin/memory-profile": {
          target: memoryProfileTarget,
          changeOrigin: true,
          rewrite: () => "/memory-profile",
        },
        "/api/admin": {
          target: adminTarget,
          changeOrigin: true,
          rewrite: (requestPath) => requestPath.replace(/^\/api\/admin/, ""),
        },
        "/api/data": {
          target: dataTarget,
          changeOrigin: true,
          rewrite: (requestPath) => requestPath.replace(/^\/api\/data/, ""),
        },
      },
      watch: {
        usePolling: true,
      },
    },
    resolve: {
      alias: {
        "@": path.resolve(__dirname, "./src"),
      },
    },
    clearScreen: false,
  }
})
