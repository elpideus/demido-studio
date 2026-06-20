import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import "highlight.js/styles/github-dark-dimmed.css";
import App from "./App";

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props)
    this.state = { error: null }
  }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 32, fontFamily: 'monospace', color: '#f87171', background: '#0d0d0f', height: '100vh' }}>
          <h2 style={{ marginBottom: 16 }}>App crashed</h2>
          <pre style={{ whiteSpace: 'pre-wrap', fontSize: 13 }}>{this.state.error.stack ?? String(this.state.error)}</pre>
          <p style={{ marginTop: 16, color: '#9ca3af' }}>Open devtools with F12 for full details.</p>
        </div>
      )
    }
    return this.props.children
  }
}

// Debug helper: in devtools run window.debugModels('provider-id') to see raw /v1/models JSON
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).debugModels = async (providerId: string) => {
    const { invoke } = await import('@tauri-apps/api/core')
    const raw = await invoke<string>('raw_provider_models_json', { providerId })
    console.log(JSON.parse(raw))
    return JSON.parse(raw)
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
