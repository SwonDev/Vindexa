import { Slot } from "radix-ui";
import type * as React from "react";
import { forwardRef } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { useReducedMotion } from "./use-reduced-motion";

/**
 * - `idle`: no hay arrastre en curso.
 * - `active`: hay un arrastre y esta zona lo admite, pero el puntero no está encima.
 * - `over`: el puntero está encima y soltar aquí funcionará.
 * - `rejected`: soltar aquí no está permitido.
 */
export type DropState = "idle" | "active" | "over" | "rejected";

export interface DragFeedbackSurfaceProps extends React.ComponentPropsWithoutRef<"div"> {
  state: DropState;
  /** Aplica el realce al hijo único en vez de envolverlo en un `div`. */
  asChild?: boolean | undefined;
  /** Número de elementos arrastrados. Solo se muestra con `active` u `over`. */
  count?: number | undefined;
  /** Etiqueta breve de lo que ocurre al soltar, p. ej. «Mover a Pendientes». */
  hint?: string | undefined;
}

/**
 * Realce de una zona de destino de arrastre.
 *
 * Reutiliza el lenguaje que ya usa la barra lateral de la biblioteca: cian para
 * «esta zona admite lo que arrastras», lima para «suelta aquí» y rojo con
 * trama diagonal para «aquí no». El único movimiento es una escala de 1.004 al
 * pasar por encima —cuatro milésimas, suficiente para notar el enganche sin
 * tapar la fila vecina— y desaparece con movimiento reducido, donde el estado
 * queda expresado solo por borde y relleno.
 *
 * Todo el realce es CSS sobre `data-drop-state`, así que puede haber decenas de
 * destinos activos a la vez sin coste de JavaScript por fotograma.
 *
 * No emite anuncios de accesibilidad: de eso se encarga el propio `@dnd-kit`
 * con sus `announcements`, y duplicarlos haría hablar dos veces al lector.
 */
export const DragFeedbackSurface = forwardRef<HTMLDivElement, DragFeedbackSurfaceProps>(
  function DragFeedbackSurface(
    { state, asChild = false, count, hint, className, children, ...props },
    ref,
  ) {
    const reducedMotion = useReducedMotion();
    const showBadge = (state === "active" || state === "over") && (count ?? 0) > 1;
    const showHint = Boolean(hint) && state !== "idle";
    const surfaceProps = {
      className: cn("vx-drop-surface", className),
      "data-slot": "drag-feedback-surface",
      "data-drop-state": state,
      "data-motion": reducedMotion ? "off" : "on",
      ...props,
    };

    // Con `asChild` el destino recibe un único hijo: el contador y la pista
    // necesitan el envoltorio propio, así que ese modo solo aporta el realce.
    if (asChild) {
      return (
        <Slot.Root ref={ref} {...surfaceProps}>
          {children}
        </Slot.Root>
      );
    }

    return (
      <div ref={ref} {...surfaceProps}>
        {children}
        {showBadge ? (
          <span className="vx-drop-surface__count" aria-hidden="true">
            {count}
          </span>
        ) : null}
        {showHint ? (
          <span className="vx-drop-surface__hint" aria-hidden="true">
            {hint}
          </span>
        ) : null}
      </div>
    );
  },
);
