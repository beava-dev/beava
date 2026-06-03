import * as React from "react"

import { InstanceHeader } from "@/components/instance-header"
import { NavMain } from "@/components/nav-main"
import { ThemeSwitcher } from "@/components/theme-switcher"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarRail,
} from "@/components/ui/sidebar"
import { routes } from "@/lib/routes"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  Bug01Icon,
  ChartBarLineIcon,
  Home01Icon,
  TableIcon,
} from "@hugeicons/core-free-icons"

const navItems = [
  {
    title: "Overview",
    url: routes.overview,
    icon: (
      <HugeiconsIcon icon={Home01Icon} strokeWidth={2} className="size-4 shrink-0" />
    ),
  },
  {
    title: "Metrics",
    url: routes.metrics,
    icon: (
      <HugeiconsIcon
        icon={ChartBarLineIcon}
        strokeWidth={2}
        className="size-4 shrink-0"
      />
    ),
  },
  {
    title: "Features",
    url: routes.features,
    icon: (
      <HugeiconsIcon icon={TableIcon} strokeWidth={2} className="size-4 shrink-0" />
    ),
  },
  {
    title: "Debug",
    url: routes.debug,
    icon: (
      <HugeiconsIcon icon={Bug01Icon} strokeWidth={2} className="size-4 shrink-0" />
    ),
  },
]

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  return (
    <Sidebar collapsible="icon" {...props}>
      <SidebarHeader>
        <InstanceHeader />
      </SidebarHeader>
      <SidebarContent>
        <NavMain items={navItems} />
      </SidebarContent>
      <SidebarFooter>
        <ThemeSwitcher />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  )
}
