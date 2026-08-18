// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
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
      familyAvailability: "confirmed",
      familySort: "updatedDesc",
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
      familyAvailability: "confirmed",
      familySort: "updatedDesc",
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

  it("restaura búsqueda, alcance, scroll y secciones tras reiniciar la app", async () => {
    const scope = { kind: "status", id: "playing", label: "Jugando" } as const;
    writeLibrarySession({
      scope,
      query: "aventura",
      sort: "lastPlayed",
      randomSeed: 27,
      view: "compact",
      familyAvailability: "confirmed",
      familySort: "updatedDesc",
      filters: { tracking: true },
    });
    writeLibraryScroll(scope, 480);
    writeLibrarySectionExpanded("collections", false);

    vi.resetModules();
    const fresh = await import("@/features/library/library-session");

    expect(fresh.readLibrarySession()).toMatchObject({
      scope: { kind: "status", id: "playing", label: "Jugando" },
      query: "aventura",
      sort: "lastPlayed",
      view: "compact",
      filters: { tracking: true },
    });
    expect(fresh.readLibraryScroll(scope)).toBe(480);
    expect(fresh.readLibrarySectionExpanded("collections")).toBe(false);
    fresh.resetLibrarySessionForTests();
  });

  it("ignora un almacenamiento corrupto y arranca con la sesión limpia", async () => {
    window.localStorage.setItem("vindexa:library-session:v1", "{json roto");

    vi.resetModules();
    const fresh = await import("@/features/library/library-session");

    expect(fresh.readLibrarySession().scope.kind).toBe("all");
    expect(fresh.readLibraryScroll({ kind: "all", label: "Todos" })).toBe(0);
    fresh.resetLibrarySessionForTests();
  });
});
