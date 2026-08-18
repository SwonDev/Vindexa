import { animate } from "motion/react";
import type * as React from "react";
import { forwardRef, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { DURATION, EASE_OUT } from "./motion-tokens";
import { useReducedMotion } from "./use-reduced-motion";

type NumberFormatter = (value: number) => string;

const defaultFormatter = new Intl.NumberFormat("es-ES", { maximumFractionDigits: 0 });

function formatDefault(value: number): string {
  return defaultFormatter.format(value);
}

export interface AnimatedNumberProps
  extends Omit<React.ComponentPropsWithoutRef<"span">, "children"> {
  /** Cifra de destino. Cada cambio dispara una transición desde la anterior. */
  value: number;
  /**
   * Formateo del valor visible. Por defecto, entero con separador de millares
   * en español. Pásale `formatPlaytime` de `@/lib/format` para minutos jugados.
   */
  format?: NumberFormatter | undefined;
  /** Duración en segundos. Se recorta al rango permitido (120–260 ms). */
  duration?: number | undefined;
  /** Desactiva la interpolación sin perder la reserva de ancho. */
  disabled?: boolean | undefined;
}

function clampDuration(duration: number | undefined): number {
  if (duration === undefined) return DURATION.base;
  return Math.min(Math.max(duration, DURATION.instant), DURATION.slow);
}

/**
 * Cifra que transiciona entre valores sin saltos de ancho.
 *
 * El ancho lo reserva un doble oculto con el texto **final**, de modo que la
 * caja adopta su tamaño definitivo en el primer fotograma y solo los dígitos se
 * mueven. Junto con `tabular-nums`, ningún elemento vecino se desplaza mientras
 * cuenta: es seguro dentro de filas virtualizadas y de barras de estado.
 *
 * El valor accesible es el del intervalo visible, que al terminar coincide
 * exactamente con `value`. Con movimiento reducido no hay intervalo: se pinta
 * directamente el destino.
 */
export const AnimatedNumber = forwardRef<HTMLSpanElement, AnimatedNumberProps>(
  function AnimatedNumber({ value, format, duration, disabled = false, className, ...props }, ref) {
    const reducedMotion = useReducedMotion();
    const formatValue = format ?? formatDefault;
    const [display, setDisplay] = useState(value);
    const previous = useRef(value);
    const skipMotion = reducedMotion || disabled;

    useEffect(() => {
      const from = previous.current;
      previous.current = value;
      if (from === value) return;
      if (skipMotion) {
        setDisplay(value);
        return;
      }
      const controls = animate(from, value, {
        duration: clampDuration(duration),
        ease: EASE_OUT,
        onUpdate: (latest) => setDisplay(latest),
        onComplete: () => setDisplay(value),
      });
      return () => controls.stop();
    }, [duration, skipMotion, value]);

    const target = formatValue(value);
    const visible = skipMotion ? target : formatValue(display);

    return (
      <span
        ref={ref}
        className={cn("vx-animated-number", className)}
        data-slot="animated-number"
        data-value={value}
        data-settled={visible === target}
        {...props}
      >
        <span className="vx-animated-number__reserve" aria-hidden="true">
          {target}
        </span>
        <span className="vx-animated-number__value" data-slot="animated-number-value">
          {visible}
        </span>
      </span>
    );
  },
);
