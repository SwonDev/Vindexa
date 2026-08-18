import { createPortal } from "react-dom";

interface OverlayProps {
  /** El velo solo existe mientras la capa que lo motiva está abierta. */
  open: boolean;
}

/**
 * Velo de las capas superpuestas.
 *
 * Diálogos, ficha y paleta traen el suyo de la primitiva y `src/index.css` lo
 * iguala. Este componente cubre el caso que no lo trae —un popover que se
 * comporta como capa modal— para que ninguna superposición quede sin separar
 * del fondo. No intercepta el puntero: el cierre lo sigue gobernando la capa.
 */
export function Overlay({ open }: OverlayProps) {
  if (!open || typeof document === "undefined") return null;
  return createPortal(<div className="overlay-scrim" aria-hidden="true" />, document.body);
}
