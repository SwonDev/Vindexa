import { describe, expect, it } from "vitest";
import {
  activeLibraryFilterCount,
  filterChips,
  type LibraryFilters,
  normalizeLibraryFilters,
} from "@/features/library/library-filters";

const filters: LibraryFilters = {
  statusId: "playing",
  installed: false,
  neverPlayed: true,
  minPlaytimeMinutes: 120,
  maxProgress: 75,
  genre: "Acción",
  releaseFrom: "2024-01-01",
  tracking: false,
};

describe("filtros combinables de biblioteca", () => {
  it("cuenta valores false y cero como filtros activos", () => {
    expect(activeLibraryFilterCount({ installed: false, tracking: false, minProgress: 0 })).toBe(3);
  });

  it("normaliza rangos, strings vacíos y límites antes de consultar SQLite", () => {
    expect(
      normalizeLibraryFilters({
        genre: "  Acción  ",
        tagId: " ",
        minProgress: -20,
        maxProgress: 180,
        minRating: 0,
        maxRating: 14,
        minPlaytimeMinutes: -5,
      }),
    ).toEqual({
      genre: "Acción",
      minProgress: 0,
      maxProgress: 100,
      minRating: 1,
      maxRating: 10,
      minPlaytimeMinutes: 0,
    });
  });

  it("genera chips completos y cada chip elimina sólo su filtro", () => {
    const chips = filterChips(filters, {
      statuses: [{ id: "playing", name: "Jugando ahora" }],
      collections: [],
      tags: [],
    });

    expect(chips.map((chip) => chip.label)).toEqual([
      "Estado: Jugando ahora",
      "No instalados",
      "Nunca jugados",
      "Horas: desde 2 h",
      "Progreso: hasta 75 %",
      "Género: Acción",
      "Lanzamiento: desde 01/01/2024",
      "Sin seguimiento",
    ]);
    expect(chips.find((chip) => chip.key === "installed")?.remove(filters)).toEqual({
      ...filters,
      installed: undefined,
    });
  });
});
