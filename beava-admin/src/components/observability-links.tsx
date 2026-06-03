import { CollapsibleSection } from "@/components/collapsible-section"
import { beavaConfig } from "@/lib/config"

function ExternalLink({ href, label }: { href: string; label: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="text-sm text-primary underline-offset-4 hover:underline"
    >
      {label}
    </a>
  )
}

export function ObservabilityLinks() {
  const links = [
    beavaConfig.grafanaUrl
      ? { href: beavaConfig.grafanaUrl, label: "Grafana" }
      : null,
    beavaConfig.prometheusUrl
      ? { href: beavaConfig.prometheusUrl, label: "Prometheus" }
      : null,
  ].filter((link): link is { href: string; label: string } => link !== null)

  if (links.length === 0) {
    return null
  }

  return (
    <CollapsibleSection
      variant="panel"
      title="External observability"
      description="Grafana and Prometheus when configured via env."
    >
      <p className="text-sm text-muted-foreground">
        {links.map((link, index) => (
          <span key={link.href}>
            {index > 0 ? " · " : null}
            <ExternalLink href={link.href} label={link.label} />
          </span>
        ))}
      </p>
    </CollapsibleSection>
  )
}
