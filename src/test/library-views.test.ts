import { describe, expect, it } from "vitest";
import type { LibraryFilters } from "@/features/library/library-filters";
import {
  combinedPresentation,
  combineViews,
  describeView,
  intersectFilters,
  normalizeQuery,
  queryMatchesView,
  type SavedLibraryView,
  type SavedViewQuery,
  toggleViewInStack,
} from "@/features/library/library-views";

function view(name: string, query: SavedViewQuery, id = name): SavedLibraryView {
  return {
    id,
    name,
    description: "",
    icon: "bookmark",
    accent: "cyan",
    query,
    pinned: false,
    position: 0,
    lastUsedAt: null,
    useCount: 0,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

describe("intersectFilters", () => {
  it("une campos que no se solapan", () => {
    const { filters, conflicts } = intersectFilters({ installed: true }, { genre: "RPG" });
    expect(filters).toEqual({ installed: true, genre: "RPG" });
    expect(conflicts).toHaveLength(0);
  });

  it("estrecha los rangos numéricos por ambos extremos", () => {
    const { filters, conflicts } = intersectFilters(
      { minRating: 6, maxRating: 10 },
      { minRating: 8, maxRating: 9 },
    );
    expect(filters).toEqual({ minRating: 8, maxRating: 9 });
    expect(conflicts).toHaveLength(0);
  });

  it("hereda el extremo que solo define una de las dos vistas", () => {
    const { filters } = intersectFilters({ minProgress: 20 }, { maxProgress: 80 });
    expect(filters).toEqual({ minProgress: 20, maxProgress: 80 });
  });

  it("declara conflicto cuando un campo de valor único difiere", () => {
    const { filters, conflicts } = intersectFilters(
      { statusId: "playing" },
      { statusId: "finished" },
    );
    // Manda la vista recién aplicada, pero el descarte queda por escrito.
    expect(filters.statusId).toBe("finished");
    expect(conflicts).toEqual([
      {
        field: "statusId",
        label: "Estado",
        discarded: "playing",
        kept: "finished",
        reason: "single-value",
      },
    ]);
  });

  it("traduce los booleanos del conflicto a lenguaje llano", () => {
    const { conflicts } = intersectFilters({ installed: true }, { installed: false });
    expect(conflicts[0]).toMatchObject({ discarded: "sí", kept: "no" });
  });

  it("no inventa conflicto si ambas piden lo mismo", () => {
    const { conflicts } = intersectFilters({ statusId: "playing" }, { statusId: "playing" });
    expect(conflicts).toHaveLength(0);
  });

  it("avisa cuando la intersección de un rango queda vacía", () => {
    const { filters, conflicts } = intersectFilters(
      { minRating: 9, maxRating: 10 },
      { minRating: 1, maxRating: 3 },
    );
    expect(filters).toEqual({ minRating: 1, maxRating: 3 });
    expect(conflicts).toEqual([
      {
        field: "minRating",
        label: "Nota",
        discarded: "9 – 10",
        kept: "1 – 3",
        reason: "empty-range",
      },
    ]);
  });

  it("interseca también rangos de fecha en ISO", () => {
    const { filters } = intersectFilters(
      { releaseFrom: "2020-01-01", releaseTo: "2026-12-31" },
      { releaseFrom: "2023-01-01" },
    );
    expect(filters).toEqual({ releaseFrom: "2023-01-01", releaseTo: "2026-12-31" });
  });

  it("descarta los campos que quedan vacíos", () => {
    // El `undefined` explícito llega de una vista guardada en disco, donde el
    // campo existe con el valor perdido. El tipo no lo admite a propósito; la
    // función tiene que sobrevivirlo igual.
    const { filters } = intersectFilters({ genre: "" }, {
      category: undefined,
    } as unknown as LibraryFilters);
    expect(filters).toEqual({});
  });
});

describe("combineViews", () => {
  it("apila tres vistas en una sola consulta", () => {
    const combined = combineViews([
      view("Instalados", { filters: { installed: true } }),
      view("Buenos", { filters: { minRating: 8 } }),
      view("RPG", { filters: { genre: "RPG" } }),
    ]);
    expect(combined.filters).toEqual({ installed: true, minRating: 8, genre: "RPG" });
    expect(combined.conflicts).toHaveLength(0);
  });

  it("junta las búsquedas sin repetirlas", () => {
    const combined = combineViews([
      view("A", { search: "souls" }),
      view("B", { search: "souls" }),
      view("C", { search: "remake" }),
    ]);
    expect(combined.search).toBe("souls remake");
  });

  it("acumula los conflictos de toda la pila", () => {
    const combined = combineViews([
      view("En curso", { filters: { statusId: "playing" } }),
      view("Terminados", { filters: { statusId: "finished" } }),
      view("Pendientes", { filters: { statusId: "backlog" } }),
    ]);
    expect(combined.conflicts).toHaveLength(2);
    expect(combined.filters.statusId).toBe("backlog");
  });

  it("una pila vacía no filtra nada", () => {
    expect(combineViews([])).toEqual({ filters: {}, search: "", conflicts: [] });
  });
});

describe("combinedPresentation", () => {
  it("la última vista decide orden, agrupación y presentación", () => {
    const result = combinedPresentation([
      view("A", { sort: "alphabetical", grouping: "initial", view: "grid" }),
      view("B", { sort: "recentlyAdded" }),
      view("C", { view: "list" }),
    ]);
    expect(result).toEqual({ sort: "recentlyAdded", grouping: "initial", view: "list" });
  });
});

describe("toggleViewInStack", () => {
  it("añade al final y respeta el orden de llegada", () => {
    expect(toggleViewInStack(["a"], "b")).toEqual(["a", "b"]);
  });

  it("quita sin alterar el resto", () => {
    expect(toggleViewInStack(["a", "b", "c"], "b")).toEqual(["a", "c"]);
  });
});

describe("queryMatchesView", () => {
  const saved = view("Instalados", {
    search: "elden",
    sort: "alphabetical",
    filters: { installed: true, minRating: 8 },
  });

  it("reconoce la misma consulta aunque cambie el orden de las claves", () => {
    expect(
      queryMatchesView(
        { search: "elden", sort: "alphabetical", filters: { minRating: 8, installed: true } },
        saved,
      ),
    ).toBe(true);
  });

  it("ignora los espacios sobrantes de la búsqueda", () => {
    expect(
      queryMatchesView(
        { search: "  elden  ", sort: "alphabetical", filters: { installed: true, minRating: 8 } },
        saved,
      ),
    ).toBe(true);
  });

  it("detecta que la consulta cambió", () => {
    expect(
      queryMatchesView(
        { search: "elden", sort: "alphabetical", filters: { installed: true, minRating: 9 } },
        saved,
      ),
    ).toBe(false);
  });

  it("no confunde un filtro vacío con uno puesto", () => {
    expect(normalizeQuery({ filters: { genre: "" } })).toBe(normalizeQuery({ filters: {} }));
  });
});

describe("describeView", () => {
  it("traduce los identificadores a nombres legibles", () => {
    const summary = describeView(
      view("Mi vista", { filters: { statusId: "playing", collectionId: "col-1" } }),
      {
        statuses: new Map([["playing", "En curso"]]),
        collections: new Map([["col-1", "Para el finde"]]),
      },
    );
    expect(summary).toBe("En curso · Para el finde");
  });

  it("distingue instalados de no instalados", () => {
    expect(describeView(view("A", { filters: { installed: false } }), {})).toBe("sin instalar");
    expect(describeView(view("B", { filters: { installed: true } }), {})).toBe("instalados");
  });

  it("una vista sin filtros lo dice con claridad", () => {
    expect(describeView(view("Todo", {}), {})).toBe("Toda la biblioteca");
  });

  it("entrecomilla la búsqueda guardada", () => {
    expect(describeView(view("A", { search: "souls", filters: { genre: "RPG" } }), {})).toBe(
      "«souls» · RPG",
    );
  });
});
