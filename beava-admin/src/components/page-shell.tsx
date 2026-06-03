import type { ReactNode } from "react"

type PageShellProps = {
  title: string
  description?: string
  headerExtra?: ReactNode
  children: ReactNode
}

export function PageShell({
  title,
  description,
  headerExtra,
  children,
}: PageShellProps) {
  return (
    <div className="flex flex-1 flex-col gap-6 p-6">
      <header className="space-y-1">
        <h1 className="font-heading text-2xl font-semibold tracking-tight">
          {title}
        </h1>
        {description ? (
          <p className="text-sm text-muted-foreground">{description}</p>
        ) : null}
        {headerExtra}
      </header>
      {children}
    </div>
  )
}
