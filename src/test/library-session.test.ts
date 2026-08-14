import { afterEach, describe, expect, it } from "vitest";
import {
  libraryScopeKey,
  readLibraryScroll,
  readLibrarySectionExpanded,
  readLibrarySession,
  resetLibrarySessionForTests,
  writeLibraryScroll,
  writeLibrarySectionExpanded,
  writeLibrarySession,
} from "@/features/library/library-session";

afterEach(resetLibrarySessionForTests);

describe("sesión de biblioteca", () => {
  it("conserva búsqueda, alcance, filtros, orden y vista entre montajes", () => {
    writeLibrarySession({
      scope: { kind: "status", id: "playing", label: "Jugando" },
      query: "aventura",
      sort: "lastPlayed",
      randomSeed: 27,
      view: "compact",
      filters: {
        installed: false,
        tracking: true,
        minPlaytimeMinutes: 120,
        genre: "Acción",
        releaseFrom: "2024-01-01",
      },
    });

    expect(readLibrarySession()).toEqual({
      scope: { kind: "status", id: "playing", label: "Jugando" },
      query: "aventura",
      sort: "lastPlayed",
      randomSeed: 27,
      view: "compact",
      filters: {
        installed: false,
        tracking: true,
        minPlaytimeMinutes: 120,
        genre: "Acción",
        releaseFrom: "2024-01-01",
      },
    });
  });

  it("restaura el desplazamiento de cada alcance sin mezclarlos", () => {
    const all = { kind: "all", label: "Todos" } as const;
    const installed = { kind: "installed", label: "Instalados" } as const;
    writeLibraryScroll(all, 812.4);
    writeLibraryScroll(installed, 144);

    expect(libraryScopeKey(all)).toBe("all:all");
    expect(readLibraryScroll(all)).toBe(812);
    expect(readLibraryScroll(installed)).toBe(144);
  });

  it("conserva el colapsado accesible de las secciones durante la sesión", () => {
    expect(readLibrarySectionExpanded("statuses")).toBe(true);
    writeLibrarySectionExpanded("statuses", false);
    expect(readLibrarySectionExpanded("statuses")).toBe(false);
  });
});
