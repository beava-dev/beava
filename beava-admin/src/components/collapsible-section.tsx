import type { ReactNode } from "react"
import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { cn } from "@/lib/utils"

type CollapsibleSectionProps = {
  title: ReactNode
  description?: ReactNode
  defaultOpen?: boolean
  action?: ReactNode
  children: ReactNode
  className?: string
  contentClassName?: string
  variant?: "card" | "panel"
}

function CollapsibleChevron() {
  return (
    <HugeiconsIcon
      icon={ArrowDown01Icon}
      strokeWidth={2}
      className="size-4 shrink-0 text-muted-foreground transition-transform"
    />
  )
}

export function CollapsibleSection({
  title,
  description,
  defaultOpen = false,
  action,
  children,
  className,
  contentClassName,
  variant = "card",
}: CollapsibleSectionProps) {
  if (variant === "panel") {
    return (
      <Collapsible
        defaultOpen={defaultOpen}
        className={cn(
          "rounded-lg border border-border bg-muted/30",
          className
        )}
      >
        <CollapsibleTrigger className="flex w-full cursor-pointer items-start gap-3 px-4 py-3 text-left [&[data-state=open]>svg]:rotate-180">
          <div className="min-w-0 flex-1 space-y-0.5">
            <p className="text-sm font-medium text-foreground">{title}</p>
            {description ? (
              <p className="text-xs text-muted-foreground">{description}</p>
            ) : null}
          </div>
          {action ? <div className="shrink-0">{action}</div> : null}
          <HugeiconsIcon
            icon={ArrowDown01Icon}
            strokeWidth={2}
            className="size-4 shrink-0 text-muted-foreground transition-transform"
          />
        </CollapsibleTrigger>
        <CollapsibleContent className="border-t border-border px-4 pb-4 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:animate-in data-[state=open]:fade-in-0">
          <div className={cn("pt-3", contentClassName)}>{children}</div>
        </CollapsibleContent>
      </Collapsible>
    )
  }

  return (
    <Collapsible defaultOpen={defaultOpen} className={className}>
      <Card>
        <CardHeader className="pb-0">
          <CollapsibleTrigger className="col-span-2 flex w-full cursor-pointer items-start gap-3 text-left [&[data-state=open]_svg]:rotate-180">
            <div className="min-w-0 flex-1 space-y-1">
              <CardTitle className="text-sm font-medium">{title}</CardTitle>
              {description ? (
                <CardDescription>{description}</CardDescription>
              ) : null}
            </div>
            {action ? (
              <CardAction className="row-span-1 self-center">{action}</CardAction>
            ) : null}
            <CollapsibleChevron />
          </CollapsibleTrigger>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className={contentClassName}>{children}</CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  )
}
