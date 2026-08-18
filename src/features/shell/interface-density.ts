import { createContext, useContext } from "react";
import type { AppPreferences } from "@/lib/types";

export type InterfaceDensity = AppPreferences["density"];

export const DENSITY_METRICS = {
  /*
   * `gridBody` es el alto que la fila reserva por debajo de la carátula: el
   * cuerpo de texto **más** el hueco hasta la fila siguiente. Se quedó corto y
   * las tarjetas invadían 7,8 px la fila de abajo, lo que hacía que el anillo
   * de selección tocase la carátula inferior. Medido con
   * `tests/e2e/layout-integrity.spec.ts`, que falla si el hueco baja de 12 px.
   */
  compact: { listRow: 58, compactRow: 38, gridBody: 96, gridGap: 16, gridPadding: 24 },
  comfortable: { listRow: 70, compactRow: 44, gridBody: 116, gridGap: 20, gridPadding: 32 },
} as const satisfies Record<
  InterfaceDensity,
  {
    listRow: number;
    compactRow: number;
    gridBody: number;
    gridGap: number;
    gridPadding: number;
  }
>;

export const InterfaceDensityContext = createContext<InterfaceDensity>("compact");

export function useInterfaceDensity(): InterfaceDensity {
  return useContext(InterfaceDensityContext);
}

/**
 * Ancho mínimo de una carátula en la rejilla. Por debajo, la portada deja de
 * leerse de un vistazo y el título del pie empieza a partirse.
 */
export const GRID_TILE_MIN_WIDTH = 176;

/**
 * Columnas que caben de verdad en un ancho dado.
 *
 * Los huecos y el relleno entran en la cuenta: `n` columnas gastan `n - 1`
 * huecos más el relleno del contenedor. Sin descontarlos, la fórmula devuelve
 * una columna de más y **todas** las carátulas encogen por debajo del mínimo,
 * de modo que ensanchar la ventana las hacía más pequeñas en lugar de mayores.
 */
export function getGridColumns(width: number, density: InterfaceDensity): number {
  const metrics = DENSITY_METRICS[density];
  const usable = width - metrics.gridPadding + metrics.gridGap;
  const step = GRID_TILE_MIN_WIDTH + metrics.gridGap;
  return Math.max(2, Math.floor(usable / step));
}

export function getVirtualGridGeometry(
  width: number,
  columns: number,
  itemCount: number,
  density: InterfaceDensity,
) {
  const safeColumns = Math.max(1, Math.floor(columns));
  const metrics = DENSITY_METRICS[density];
  const cardWidth =
    (width - metrics.gridPadding - (safeColumns - 1) * metrics.gridGap) / safeColumns;
  const rowHeight = Math.ceil(cardWidth * 1.5 + metrics.gridBody);
  const rowCount = Math.ceil(itemCount / safeColumns);
  return {
    rowCount,
    rowHeight,
    totalHeight: rowCount * rowHeight,
    rowStart: (index: number) => Math.max(0, Math.floor(index)) * rowHeight,
  };
}
