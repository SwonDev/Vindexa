import { DENSITY_METRICS, type InterfaceDensity } from "@/features/shell/interface-density";

/**
 * Alto de fila de las dos vistas tabulares.
 *
 * La ultracompacta renuncia a carátula y a barra de progreso dibujada —el
 * porcentaje va en texto— porque su único propósito es ver de un vistazo el
 * máximo de biblioteca posible: en una pantalla de 900 px caben cerca de
 * treinta filas.
 */
const ULTRA_COMPACT_ROW = {
  compact: 26,
  comfortable: 30,
} as const satisfies Record<InterfaceDensity, number>;

export function listRowHeight(density: InterfaceDensity, ultraCompact: boolean): number {
  return ultraCompact ? ULTRA_COMPACT_ROW[density] : DENSITY_METRICS[density].listRow;
}

/**
 * Alto del recuadro visible de la fila.
 *
 * El píxel que se descuenta es la holgura entre filas: el paso completo lo
 * reserva el virtualizador, así que el recuadro tiene que quedarse justo por
 * debajo para que el separador respire y dos filas nunca se solapen.
 */
export function listRowBoxHeight(rowHeight: number): number {
  return Math.max(1, rowHeight - 1);
}
