import { describe, expect, it } from "vitest";
import { getVirtualGridGeometry } from "@/features/shell/interface-density";

describe("geometría virtual por densidad", () => {
  it.each([
    { density: "compact" as const, expectedRowHeight: 324 },
    { density: "comfortable" as const, expectedRowHeight: 337 },
  ])("mantiene filas contiguas en modo $density", ({ density, expectedRowHeight }) => {
    const geometry = getVirtualGridGeometry(900, 5, 11, density);

    expect(geometry.rowCount).toBe(3);
    expect(geometry.rowHeight).toBe(expectedRowHeight);
    expect([0, 1, 2].map(geometry.rowStart)).toEqual([0, expectedRowHeight, expectedRowHeight * 2]);
    expect(geometry.totalHeight).toBe(expectedRowHeight * 3);
    expect(geometry.rowStart(2) + geometry.rowHeight).toBe(geometry.totalHeight);
  });
});
