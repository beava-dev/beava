import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import { useTheme } from "@/components/theme-provider"
import { HugeiconsIcon } from "@hugeicons/react"
import { ComputerIcon, Moon02Icon, Sun01Icon } from "@hugeicons/core-free-icons"

const themeOptions = [
  { value: "light" as const, label: "Light", icon: Sun01Icon },
  { value: "dark" as const, label: "Dark", icon: Moon02Icon },
  { value: "system" as const, label: "System", icon: ComputerIcon },
]

function getThemeOption(theme: (typeof themeOptions)[number]["value"]) {
  return (
    themeOptions.find((option) => option.value === theme) ?? themeOptions[0]
  )
}

export function ThemeSwitcher() {
  const { theme, setTheme } = useTheme()
  const activeTheme = getThemeOption(theme)

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton tooltip={`Theme: ${activeTheme.label}`}>
              <HugeiconsIcon
                icon={activeTheme.icon}
                strokeWidth={2}
                className="size-4 shrink-0"
              />
              <span>{activeTheme.label} Theme</span>
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="right" align="end" className="min-w-36">
            <DropdownMenuRadioGroup
              value={theme}
              onValueChange={(value) => {
                if (
                  value === "light" ||
                  value === "dark" ||
                  value === "system"
                ) {
                  setTheme(value)
                }
              }}
            >
              {themeOptions.map((option) => (
                <DropdownMenuRadioItem key={option.value} value={option.value}>
                  <HugeiconsIcon
                    icon={option.icon}
                    strokeWidth={2}
                    className="size-4 shrink-0"
                  />
                  {option.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
