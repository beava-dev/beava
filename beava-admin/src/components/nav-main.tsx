import { NavLink, useLocation } from "react-router-dom"

import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import { routes } from "@/lib/routes"

export type NavItem = {
  title: string
  url: string
  icon?: React.ReactNode
}

export interface NavMain {
  items: NavItem[]
}

export function NavMain(props: NavMain) {
  const { pathname } = useLocation()

  return (
    <SidebarGroup>
      <SidebarGroupLabel>Platform</SidebarGroupLabel>
      <SidebarMenu className="space-y-1">
        {props.items.map((item) => {
          const isActive =
            item.url === routes.overview
              ? pathname === routes.overview
              : pathname.startsWith(item.url)

          return (
            <SidebarMenuItem key={item.url}>
              <SidebarMenuButton
                asChild
                tooltip={item.title}
                isActive={isActive}
              >
                <NavLink to={item.url}>
                  {item.icon}
                  <span>{item.title}</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          )
        })}
      </SidebarMenu>
    </SidebarGroup>
  )
}
