import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "./index.css"
import App from "./App.tsx"
import { DevErrorBoundary } from "@/components/dev-error-boundary.tsx"
import { ThemeProvider } from "@/components/theme-provider.tsx"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <DevErrorBoundary>
      <ThemeProvider defaultTheme="light">
        <App />
      </ThemeProvider>
    </DevErrorBoundary>
  </StrictMode>
)
