import type { ReactNode } from "react"

import { cn } from "@/lib/utils"

type StatCardProps = {
  label: string
  value: ReactNode
  hint?: string
  className?: string
}

export function StatCard({ label, value, hint, className }: StatCardProps) {
  return (
    <div
      className={cn(
        "rounded-lg border border-border bg-card px-4 py-3 ring-1 ring-foreground/5",
        className
      )}
    >
      <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </p>
      <p className="mt-1 font-medium tabular-nums text-foreground">{value}</p>
      {hint ? (
        <p className="mt-1 text-xs text-muted-foreground">{hint}</p>
      ) : null}
    </div>
  )
}
