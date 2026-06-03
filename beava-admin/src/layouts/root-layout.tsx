import { Outlet } from "react-router-dom"

import { AppBreadcrumb } from "@/components/app-breadcrumb"
import { AppSidebar } from "@/components/app-sidebar"
import { Separator } from "@/components/ui/separator"
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar"
import { TooltipProvider } from "@/components/ui/tooltip"

export default function RootLayout() {
  return (
    <TooltipProvider>
      <SidebarProvider>
        <AppSidebar />
        <SidebarInset>
          <header className="sticky top-0 flex h-14 shrink-0 items-center gap-2 border-b border-border bg-background px-4 shadow">
            <SidebarTrigger />
            <Separator orientation="vertical" className="mr-2" />
            <AppBreadcrumb />
          </header>
          <main className="flex flex-1 flex-col scroll-smooth">
            <Outlet />
          </main>
        </SidebarInset>
      </SidebarProvider>
    </TooltipProvider>
  )
}
