import { createContext, useContext } from "react";
import type { AppPreferences } from "@/lib/types";

export type InterfaceDensity = AppPreferences["density"];

export const DENSITY_METRICS = {
  compact: { listRow: 58, compactRow: 38, gridBody: 72, gridGap: 10, gridPadding: 20 },
  comfortable: { listRow: 70, compactRow: 44, gridBody: 92, gridGap: 14, gridPadding: 28 },
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
