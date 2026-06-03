import type { ReactNode } from "react"

import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"
import type { PollingResource } from "@/hooks/use-polling-resource"

const successBadgeClass =
  "border-transparent bg-success text-success-foreground"

export function SuccessBadge({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return <Badge className={cn(successBadgeClass, className)}>{children}</Badge>
}

const PASSING_PROBE_STATUSES = new Set(["ok", "ready"])

function StatusDot({
  className,
  label,
}: {
  className: string
  label: string
}) {
  return (
    <span
      role="status"
      aria-label={label}
      title={label}
      className={cn("inline-block size-2.5 shrink-0 rounded-full", className)}
    />
  )
}

export function ProbeStatusBadge({ status }: { status: string }) {
  if (PASSING_PROBE_STATUSES.has(status)) {
    return <StatusDot className="bg-success" label={status} />
  }

  return (
    <span className="inline-flex items-center gap-2">
      <StatusDot className="bg-destructive" label={status} />
      <span className="text-sm text-destructive">{status}</span>
    </span>
  )
}

export function PollingStatusBadge({
  resource,
}: {
  resource: PollingResource<unknown>
}) {
  if (resource.isLoading) {
    return <Badge variant="secondary">Loading</Badge>
  }

  if (resource.isError) {
    return <Badge variant="destructive">Unreachable</Badge>
  }

  return <SuccessBadge>Live</SuccessBadge>
}
