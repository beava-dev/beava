"use client"

import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import { cn } from "@/lib/utils"

export function InstanceHeader() {
  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <SidebarMenuButton
          size="lg"
          tooltip="Beava Admin"
          className={cn(
            "h-16 data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground",
            "group-data-[collapsible=icon]:size-8! group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:gap-0"
          )}
          onClick={() => window.location.replace("/")}
        >
          <div
            className={cn(
              "flex size-12 shrink-0 items-center justify-center rounded-full border-2 border-primary",
              "group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:border-0"
            )}
          >
            <img
              src="/logo-mark.png"
              alt=""
              className="size-full rounded-full object-contain"
            />
          </div>
          <div className="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
            <span className="truncate font-medium">Beava Admin</span>
            <span className="truncate text-xs">Local Dashboard</span>
          </div>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
