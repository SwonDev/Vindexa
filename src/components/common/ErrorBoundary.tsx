import { IconAlertTriangle, IconRefresh } from "@tabler/icons-react";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@/components/ui/button";

interface Props {
  children: ReactNode;
  onReset: () => void;
}
interface State {
  error?: Error;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = {};
  static getDerivedStateFromError(error: Error): State {
    return { error };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Vindexa UI error", error, info);
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="fatal-state">
        <div className="fatal-state__panel">
          <IconAlertTriangle aria-hidden="true" size={30} />
          <div>
            <p className="eyebrow">Recuperación segura</p>
            <h1>La interfaz no pudo continuar</h1>
            <p>
              Tus datos siguen en la base local. Reinicia solo la interfaz para volver a intentarlo.
            </p>
          </div>
          <Button onClick={this.props.onReset}>
            <IconRefresh aria-hidden="true" /> Reiniciar interfaz
          </Button>
        </div>
      </main>
    );
  }
}
