import type { ReactNode } from "react"

import { PollingStatusBadge } from "@/components/status-badge"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import type { PollingResource } from "@/hooks/use-polling-resource"

type StatusCardProps<T> = {
  title: string
  description: string
  resource: PollingResource<T>
  errorMessage: string
  renderContent: (data: T) => ReactNode
}

function StatusCardSkeleton() {
  return (
    <div className="space-y-3">
      <Skeleton className="h-4 w-2/3" />
      <Skeleton className="h-4 w-1/2" />
    </div>
  )
}

export function StatusCard<T>({
  title,
  description,
  resource,
  errorMessage,
  renderContent,
}: StatusCardProps<T>) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
        <CardAction>
          <PollingStatusBadge resource={resource} />
        </CardAction>
      </CardHeader>
      <CardContent>
        {resource.isLoading && <StatusCardSkeleton />}
        {resource.isError && (
          <p className="text-sm text-destructive">
            {errorMessage}
            {resource.error?.message ? (
              <span className="mt-1 block font-mono text-xs text-muted-foreground">
                {resource.error.message}
              </span>
            ) : null}
          </p>
        )}
        {resource.isSuccess && resource.data !== undefined
          ? renderContent(resource.data)
          : null}
      </CardContent>
    </Card>
  )
}
