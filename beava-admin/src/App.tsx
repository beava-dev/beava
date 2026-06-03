import { BrowserRouter, Route, Routes } from "react-router-dom"

import RootLayout from "./layouts/root-layout"
import DebugPage from "./pages/debug"
import FeaturesPage from "./pages/features"
import MetricsPage from "./pages/metrics"
import OverviewPage from "./pages/overview"

export function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<RootLayout />}>
          <Route path="/" element={<OverviewPage />} />
          <Route path="/metrics" element={<MetricsPage />} />
          <Route path="/features" element={<FeaturesPage />} />
          <Route path="/debug" element={<DebugPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}

export default App
