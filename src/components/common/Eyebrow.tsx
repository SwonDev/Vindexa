import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

interface EyebrowProps {
  children: ReactNode;
  /**
   * El antetítulo repite en texto lo que el encabezado ya dice. Cuando solo
   * sirve de rótulo visual se oculta a los lectores de pantalla.
   */
  decorative?: boolean | undefined;
  className?: string | undefined;
}

/**
 * Antetítulo de sección.
 *
 * Un único tamaño, un único color y un único espaciado para las once pantallas:
 * es la línea corta en mayúsculas que sitúa al encabezado que va debajo.
 */
export function Eyebrow({ children, decorative, className }: EyebrowProps) {
  return (
    <p className={cn("eyebrow", className)} aria-hidden={decorative ? "true" : undefined}>
      {children}
    </p>
  );
}
