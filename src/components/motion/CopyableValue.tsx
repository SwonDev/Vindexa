import { IconAlertCircle, IconCheck, IconCopy } from "@tabler/icons-react";
import type * as React from "react";
import { forwardRef, useCallback, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { IconMorph } from "./IconMorph";
import "./motion.css";
import { useReducedMotion } from "./use-reduced-motion";

type CopyStatus = "idle" | "copied" | "error";

export interface CopyableValueProps
  extends Omit<React.ComponentPropsWithoutRef<"button">, "onCopy" | "value" | "children"> {
  /** Texto que se copia al portapapeles. */
  value: string;
  /** Texto visible. Por defecto, el propio valor. */
  display?: React.ReactNode | undefined;
  /** Nombre accesible del botón. Por defecto, «Copiar <valor>». */
  label?: string | undefined;
  /** Milisegundos que dura la confirmación antes de volver al icono de copia. */
  confirmMs?: number | undefined;
  /**
   * Implementación alternativa del copiado. Existe para poder pasar el
   * portapapeles nativo de Tauri el día que se añada el complemento, sin tocar
   * este componente.
   */
  copy?: ((value: string) => Promise<void> | void) | undefined;
  onCopied?: ((value: string) => void) | undefined;
  onCopyError?: ((error: unknown) => void) | undefined;
}

async function copyWithWebApi(value: string): Promise<void> {
  if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
    throw new Error("clipboard_unavailable");
  }
  await navigator.clipboard.writeText(value);
}

/**
 * Valor copiable con confirmación en el propio icono.
 *
 * Al copiar, el icono se cruza a una marca de verificación durante segundo y
 * medio y vuelve solo; el resultado se anuncia además en una región discreta,
 * porque un cambio de icono no lo percibe un lector de pantalla.
 *
 * El ancho del botón no cambia entre estados: el icono vive en una caja fija,
 * así que la fila no se recompone al confirmar.
 *
 * Usa el portapapeles de la plataforma web, disponible en la vista de Tauri.
 * Si algún día se instala un complemento nativo, basta con pasarlo por `copy`.
 */
export const CopyableValue = forwardRef<HTMLButtonElement, CopyableValueProps>(
  function CopyableValue(
    {
      value,
      display,
      label,
      confirmMs = 1500,
      copy,
      onCopied,
      onCopyError,
      className,
      onClick,
      ...props
    },
    ref,
  ) {
    const reducedMotion = useReducedMotion();
    const [status, setStatus] = useState<CopyStatus>("idle");
    const timer = useRef<number | undefined>(undefined);

    useEffect(
      () => () => {
        window.clearTimeout(timer.current);
      },
      [],
    );

    const scheduleReset = useCallback(() => {
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setStatus("idle"), Math.max(400, confirmMs));
    }, [confirmMs]);

    const handleClick = async (event: React.MouseEvent<HTMLButtonElement>) => {
      onClick?.(event);
      if (event.defaultPrevented) return;
      try {
        await (copy ?? copyWithWebApi)(value);
        setStatus("copied");
        onCopied?.(value);
      } catch (error) {
        setStatus("error");
        onCopyError?.(error);
      }
      scheduleReset();
    };

    const announcement =
      status === "copied" ? "Copiado" : status === "error" ? "No se pudo copiar" : "";

    return (
      <span className="vx-copyable-wrap">
        <button
          ref={ref}
          type="button"
          className={cn("vx-copyable", className)}
          data-slot="copyable-value"
          data-status={status}
          data-motion={reducedMotion ? "off" : "on"}
          aria-label={label ?? `Copiar ${value}`}
          onClick={(event) => {
            void handleClick(event);
          }}
          {...props}
        >
          <span className="vx-copyable__text">{display ?? value}</span>
          <IconMorph
            className="vx-copyable__icon"
            confirmed={status !== "idle"}
            icon={<IconCopy size={13} stroke={1.8} />}
            confirmIcon={
              status === "error" ? (
                <IconAlertCircle size={13} stroke={1.8} />
              ) : (
                <IconCheck size={13} stroke={2.2} />
              )
            }
          />
        </button>
        {/* Fuera del botón: un lector de pantalla no anuncia una región viva
            encerrada en un control con nombre accesible propio. */}
        <span className="sr-only" role="status" aria-live="polite">
          {announcement}
        </span>
      </span>
    );
  },
);
