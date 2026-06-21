import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

// React error boundaries must be class components; this is the one intentional exception to the
// functional-component convention. It keeps a single render error (e.g. a malformed record) from
// blanking the entire window, showing a recoverable message instead.
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("ScreenSearch UI error", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="app-error" role="alert">
          <h1>Something went wrong</h1>
          <p>The interface hit an unexpected error. Your captured data is unaffected.</p>
          <pre>{this.state.error.message}</pre>
          <div className="app-error-actions">
            <button type="button" onClick={() => this.setState({ error: null })}>Try again</button>
            <button type="button" onClick={() => window.location.reload()}>Reload</button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
