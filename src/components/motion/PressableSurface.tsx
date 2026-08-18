import { Slot } from "radix-ui";
import type * as React from "react";
import { forwardRef } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { useReducedMotion } from "./use-reduced-motion";

/** Elevación máxima al pasar el puntero. Más de 4 px deja de leerse como tacto. */
const MAX_LIFT_PX = 4;
/** Escala máxima. Por encima de 1.02 la fila empieza a tapar a sus vecinas. */
const MAX_SCALE = 1.02;

export interface PressableSurfaceProps extends React.ComponentPropsWithoutRef<"button"> {
  /** Aplica el comportamiento al hijo único en vez de renderizar un `button`. */
  asChild?: boolean | undefined;
  /** Píxeles que sube el elemento al pasar el puntero. 0–4, por defecto 1. */
  liftPx?: number | undefined;
  /** Escala al pasar el puntero. 1–1.02, por defecto 1.01. */
  hoverScale?: number | undefined;
  /** Escala al pulsar. Debe ser ≤ 1; por defecto 0.99. */
  pressScale?: number | undefined;
  /** Anula el realce sin perder el resto del comportamiento del botón. */
  motionDisabled?: boolean | undefined;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Respuesta táctil contenida para superficies pulsables.
 *
 * Sube un píxel al pasar el puntero y se hunde al pulsar, con `transform`
 * únicamente: nada de esto provoca `layout`, así que puede usarse dentro de una
 * fila virtualizada sin alterar su altura medida. No hay efecto magnético ni
 * seguimiento del cursor — en una lista densa, un elemento que persigue al
 * puntero convierte un clic en puntería.
 *
 * Todo el movimiento vive en CSS y desaparece con `prefers-reduced-motion`; el
 * componente además marca `data-motion="off"` para que quede explícito en el
 * DOM y sea comprobable.
 */
export const PressableSurface = forwardRef<HTMLButtonElement, PressableSurfaceProps>(
  function PressableSurface(
    {
      asChild = false,
      liftPx = 1,
      hoverScale = 1.01,
      pressScale = 0.99,
      motionDisabled = false,
      className,
      style,
      type,
      children,
      ...props
    },
    ref,
  ) {
    const reducedMotion = useReducedMotion();
    const motionOff = reducedMotion || motionDisabled;
    const Component = asChild ? Slot.Root : "button";

    return (
      <Component
        ref={ref}
        className={cn("vx-pressable", className)}
        data-slot="pressable-surface"
        data-motion={motionOff ? "off" : "on"}
        {...(asChild ? {} : { type: type ?? "button" })}
        style={
          {
            ...style,
            "--vx-press-lift": `${-clamp(liftPx, 0, MAX_LIFT_PX)}px`,
            "--vx-press-hover-scale": clamp(hoverScale, 1, MAX_SCALE),
            "--vx-press-active-scale": clamp(pressScale, 0.96, 1),
          } as React.CSSProperties
        }
        {...props}
      >
        {children}
      </Component>
    );
  },
);
