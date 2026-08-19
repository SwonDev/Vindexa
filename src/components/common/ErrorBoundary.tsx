import { IconAlertTriangle, IconCopy, IconRefresh } from "@tabler/icons-react";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { Eyebrow } from "@/components/common/Eyebrow";
import { Button } from "@/components/ui/button";

interface Props {
  children: ReactNode;
  onReset: () => void;
}
interface State {
  error?: Error;
  info?: ErrorInfo;
}

/**
 * Última red de la interfaz.
 *
 * # Por qué enseña el error
 *
 * Antes sólo lo mandaba a la consola, y en una aplicación empaquetada la
 * consola no existe: quien se encontraba esta pantalla no tenía forma de decir
 * qué había pasado, ni yo de averiguarlo. «La interfaz no pudo continuar» sin
 * más es exactamente igual de útil para todo el mundo, que es como decir nada.
 *
 * Así que el mensaje se enseña y se puede copiar entero —mensaje, pila y el
 * componente donde saltó—. Es texto de la propia aplicación: no lleva ni
 * biblioteca, ni cuenta, ni nada de quien la usa.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = {};
  static getDerivedStateFromError(error: Error): State {
    return { error };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Vindexa UI error", error, info);
    this.setState({ info });
  }

  private detail(): string {
    const { error, info } = this.state;
    return [
      error?.message ?? "Error desconocido",
      error?.stack,
      info?.componentStack ? `Componente:${info.componentStack}` : undefined,
    ]
      .filter(Boolean)
      .join("\n\n");
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="fatal-state">
        <div className="fatal-state__panel">
          <IconAlertTriangle aria-hidden="true" size={30} />
          <div>
            <Eyebrow>Recuperación segura</Eyebrow>
            <h1>La interfaz no pudo continuar</h1>
            <p>
              Tus datos siguen en la base local. Reinicia solo la interfaz para volver a intentarlo.
            </p>
            <p className="fatal-state__message">{this.state.error.message}</p>
            <details className="fatal-state__detail">
              <summary>Ver detalle técnico</summary>
              <pre>{this.detail()}</pre>
            </details>
          </div>
          <div className="fatal-state__actions">
            <Button onClick={this.props.onReset}>
              <IconRefresh aria-hidden="true" /> Reiniciar interfaz
            </Button>
            <Button
              variant="ghost"
              onClick={() => {
                navigator.clipboard?.writeText(this.detail()).catch(() => undefined);
              }}
            >
              <IconCopy aria-hidden="true" /> Copiar detalle
            </Button>
          </div>
        </div>
      </main>
    );
  }
}
