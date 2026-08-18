import { AnimatePresence, motion } from "motion/react";
import type * as React from "react";
import { forwardRef } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { EASE_OUT, TRANSITION_NONE } from "./motion-tokens";
import { useReducedMotion } from "./use-reduced-motion";

export interface IconMorphProps extends React.ComponentPropsWithoutRef<"span"> {
  /** Icono en reposo. */
  icon: React.ReactNode;
  /** Icono de confirmación. */
  confirmIcon: React.ReactNode;
  /** Cuando pasa a `true` se cruza al icono de confirmación. */
  confirmed: boolean;
  /** Lado de la caja en píxeles. Fija el hueco para que nada se mueva. */
  sizePx?: number | undefined;
}

/**
 * Cambio de icono con confirmación.
 *
 * Los dos iconos ocupan la misma caja de tamaño fijo y se cruzan en 120 ms con
 * una escala mínima (0.88 → 1). Al no medir nada ni cambiar de tamaño, no
 * desplaza el texto que tenga al lado ni altera la altura de una fila.
 *
 * Es puramente visual: `aria-hidden`. Quien lo use debe anunciar el resultado
 * por su cuenta, como hace `CopyableValue`.
 */
export const IconMorph = forwardRef<HTMLSpanElement, IconMorphProps>(function IconMorph(
  { icon, confirmIcon, confirmed, sizePx = 14, className, style, ...props },
  ref,
) {
  const reducedMotion = useReducedMotion();
  const transition = reducedMotion ? TRANSITION_NONE : { duration: 0.12, ease: EASE_OUT };

  return (
    <span
      ref={ref}
      className={cn("vx-icon-morph", className)}
      data-slot="icon-morph"
      data-confirmed={confirmed}
      aria-hidden="true"
      style={{ ...style, width: `${sizePx}px`, height: `${sizePx}px` }}
      {...props}
    >
      <AnimatePresence initial={false}>
        <motion.span
          key={confirmed ? "confirm" : "rest"}
          className="vx-icon-morph__slot"
          initial={{ opacity: 0, scale: reducedMotion ? 1 : 0.88 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: reducedMotion ? 1 : 0.88 }}
          transition={transition}
        >
          {confirmed ? confirmIcon : icon}
        </motion.span>
      </AnimatePresence>
    </span>
  );
});
