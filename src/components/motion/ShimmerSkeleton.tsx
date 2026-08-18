import type * as React from "react";
import { forwardRef } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { useReducedMotion } from "./use-reduced-motion";

/** Radio máximo permitido por el sistema de diseño para un bloque de carga. */
const MAX_RADIUS_PX = 3;

export interface ShimmerSkeletonProps extends React.ComponentPropsWithoutRef<"div"> {
  /** Ancho del bloque. Número en píxeles o cualquier medida CSS. */
  width?: number | string | undefined;
  /** Alto del bloque. Ignorado si se usa `aspectRatio`. */
  height?: number | string | undefined;
  /** Proporción real del contenido, p. ej. `"2 / 3"` para una carátula. */
  aspectRatio?: string | undefined;
  /** Radio en píxeles, recortado a 0–3 para no salirse de la geometría técnica. */
  radiusPx?: number | undefined;
  /** Número de bloques apilados. Útil para listas de filas de igual altura. */
  count?: number | undefined;
  /** Separación entre bloques cuando `count > 1`. */
  gapPx?: number | undefined;
  /** Desactiva el barrido y deja el bloque estático. */
  shimmer?: boolean | undefined;
  /**
   * Si se indica, el conjunto se anuncia como estado de carga con este texto.
   * Sin él, el esqueleto es puramente decorativo y queda fuera del árbol de
   * accesibilidad, que es lo correcto cuando ya hay otro `role="status"` cerca.
   */
  label?: string | undefined;
}

/**
 * Esqueleto de carga con la geometría real del contenido.
 *
 * Rectangular, radio máximo de 3 px y sin latido de opacidad: el barrido es un
 * pseudoelemento que solo se desplaza con `translateX`, igual que el de las
 * carátulas, de modo que un mosaico entero cuesta una capa de composición y no
 * repinta nada. Con movimiento reducido el barrido no existe.
 *
 * Reserva exactamente el hueco final —por alto o por proporción—, así que al
 * llegar los datos no hay salto y una lista virtualizada mantiene su medida.
 */
export const ShimmerSkeleton = forwardRef<HTMLDivElement, ShimmerSkeletonProps>(
  function ShimmerSkeleton(
    {
      width = "100%",
      height,
      aspectRatio,
      radiusPx = 2,
      count = 1,
      gapPx = 8,
      shimmer = true,
      label,
      className,
      style,
      ...props
    },
    ref,
  ) {
    const reducedMotion = useReducedMotion();
    const animated = shimmer && !reducedMotion;
    const blocks = Math.max(1, Math.floor(count));
    const radius = Math.min(Math.max(radiusPx, 0), MAX_RADIUS_PX);

    const blockStyle: React.CSSProperties = {
      width,
      borderRadius: `${radius}px`,
      ...(aspectRatio ? { aspectRatio } : { height: height ?? 12 }),
    };

    return (
      <div
        ref={ref}
        className={cn("vx-skeleton-group", className)}
        data-slot="shimmer-skeleton"
        data-shimmer={animated}
        style={{ ...style, gap: `${gapPx}px` }}
        {...(label
          ? { role: "status", "aria-live": "polite", "aria-busy": true }
          : { "aria-hidden": true })}
        {...props}
      >
        {label ? <span className="sr-only">{label}</span> : null}
        {Array.from({ length: blocks }, (_, index) => (
          <span
            // biome-ignore lint/suspicious/noArrayIndexKey: los bloques son idénticos y no tienen identidad propia
            key={index}
            className="vx-skeleton"
            data-slot="shimmer-skeleton-block"
            style={blockStyle}
            aria-hidden="true"
          />
        ))}
      </div>
    );
  },
);
