import { Component, type ErrorInfo, type ReactNode } from "react"
import { Button } from "./ui/button"

type DevErrorBoundaryProps = {
  children: ReactNode
}

type DevErrorBoundaryState = {
  error: Error | null
  componentStack: string
}

export class DevErrorBoundary extends Component<
  DevErrorBoundaryProps,
  DevErrorBoundaryState
> {
  state: DevErrorBoundaryState = {
    error: null,
    componentStack: "",
  }

  static getDerivedStateFromError(error: Error): DevErrorBoundaryState {
    return {
      error,
      componentStack: "",
    }
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error(error, errorInfo)
    this.setState({ componentStack: errorInfo.componentStack ?? "" })
  }

  render() {
    if (!import.meta.env.DEV || !this.state.error) {
      return this.props.children
    }

    return (
      <div
        role="alertdialog"
        aria-labelledby="dev-error-title"
        aria-describedby="dev-error-message"
        className="bg-background-950/95 fixed inset-0 z-50 overflow-auto p-6"
      >
        <div className="border-border-400/40 bg-background-950 mx-auto max-w-5xl rounded-lg border p-6 shadow-2xl">
          <div className="mb-4 flex items-start justify-between gap-4">
            <div>
              <p className="text-sm font-medium">React render error</p>
              <h1 id="dev-error-title" className="mt-1 text-2xl font-semibold">
                {this.state.error.name}
              </h1>
            </div>
            <Button onClick={() => window.location.reload()}>Reload</Button>
          </div>

          <pre
            id="dev-error-message"
            className="max-h-96 overflow-x-auto overflow-y-scroll rounded-md bg-foreground/10 p-4 font-mono text-sm leading-6 whitespace-pre-wrap"
          >
            {this.state.error.message}
            {"\n\n"}
            {this.state.error.stack}
            {this.state.componentStack
              ? `\n\n${this.state.componentStack}`
              : ""}
          </pre>
        </div>
      </div>
    )
  }
}
