import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_LOCAL_SHORTCUTS,
  DEFAULT_SHORTCUTS,
  describeShortcutRejection,
  eventToShortcut,
  findShortcutCollision,
  gameContextShortcuts,
  getReservedShortcutName,
  isBackendShortcut,
  isEditableShortcutTarget,
  isLibrarySurfaceTarget,
  LEGACY_SEARCH_SHORTCUT,
  matchesShortcut,
  readLocalShortcuts,
  requiresLibrarySurface,
  resolveShortcutEvent,
  resolveShortcuts,
  SHORTCUT_CATALOGUE,
  sectionShortcut,
  shortcutLabel,
  writeLocalShortcuts,
} from "@/features/shell/shortcuts";

function keyboardEvent(key: string, init: KeyboardEventInit = {}) {
  return new KeyboardEvent("keydown", { key, ...init });
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("atajos configurables", () => {
  it("normaliza Command en macOS y Control en otras plataformas como Mod", () => {
    expect(eventToShortcut(keyboardEvent("k", { metaKey: true }))).toBe("Mod+K");
    expect(eventToShortcut(keyboardEvent("k", { ctrlKey: true }))).toBe("Mod+K");
    expect(eventToShortcut(keyboardEvent("S", { ctrlKey: true, shiftKey: true }))).toBe(
      "Mod+Shift+S",
    );
  });

  it("detecta colisiones antes de persistir una asignación", () => {
    expect(findShortcutCollision(DEFAULT_SHORTCUTS, "planner", "Mod+1")).toBe("library");
    expect(findShortcutCollision(DEFAULT_SHORTCUTS, "planner", "Mod+2")).toBeUndefined();
    expect(getReservedShortcutName("Mod+Comma")).toBe("Abrir Ajustes");
    expect(getReservedShortcutName("Mod+K")).toBeUndefined();
  });

  it("no activa atajos mientras la persona escribe", () => {
    const input = document.createElement("input");
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");

    expect(isEditableShortcutTarget(input)).toBe(true);
    expect(isEditableShortcutTarget(editor)).toBe(true);
    expect(isEditableShortcutTarget(document.createElement("button"))).toBe(false);
  });

  it("compara el evento contra la combinación persistida", () => {
    expect(matchesShortcut(keyboardEvent("1", { metaKey: true }), "Mod+1")).toBe(true);
    expect(matchesShortcut(keyboardEvent("1"), "Mod+1")).toBe(false);
    expect(matchesShortcut(keyboardEvent("Escape"), "Escape")).toBe(true);
  });
});

describe("teclas operativas sin modificador", () => {
  it("admite Intro, Espacio, flechas, Inicio y Fin, pero no letras sueltas", () => {
    expect(eventToShortcut(keyboardEvent("Enter"))).toBe("Enter");
    expect(eventToShortcut(keyboardEvent(" "))).toBe("Space");
    expect(eventToShortcut(keyboardEvent("ArrowDown"))).toBe("ArrowDown");
    expect(eventToShortcut(keyboardEvent("Home"))).toBe("Home");
    expect(eventToShortcut(keyboardEvent("End"))).toBe("End");
    expect(eventToShortcut(keyboardEvent("Delete"))).toBe("Delete");
    // Escribir no es un atajo: ni una letra suelta ni una letra con Mayús.
    expect(eventToShortcut(keyboardEvent("a"))).toBeUndefined();
    expect(eventToShortcut(keyboardEvent("A", { shiftKey: true }))).toBeUndefined();
  });

  it("compone Mayús+flecha para extender la selección", () => {
    expect(eventToShortcut(keyboardEvent("ArrowUp", { shiftKey: true }))).toBe("Shift+ArrowUp");
    expect(matchesShortcut(keyboardEvent("ArrowUp", { shiftKey: true }), "Shift+ArrowUp")).toBe(
      true,
    );
  });

  it("reserva las teclas desnudas a la superficie de la biblioteca", () => {
    expect(requiresLibrarySurface("Enter")).toBe(true);
    expect(requiresLibrarySurface("Space")).toBe(true);
    expect(requiresLibrarySurface("Shift+ArrowDown")).toBe(true);
    expect(requiresLibrarySurface("Mod+K")).toBe(false);
    expect(requiresLibrarySurface("Alt+ArrowRight")).toBe(false);

    const browser = document.createElement("div");
    browser.className = "game-browser";
    const card = document.createElement("button");
    browser.append(card);
    document.body.append(browser);
    const outside = document.createElement("button");
    document.body.append(outside);

    expect(isLibrarySurfaceTarget(card)).toBe(true);
    expect(isLibrarySurfaceTarget(outside)).toBe(false);
    browser.remove();
    outside.remove();
  });
});

describe("compatibilidad con el validador de Rust", () => {
  it("acepta lo que `is_valid_shortcut` acepta y rechaza lo demás", () => {
    expect(isBackendShortcut("Mod+1")).toBe(true);
    expect(isBackendShortcut("Mod+Shift+S")).toBe(true);
    expect(isBackendShortcut("Escape")).toBe(true);
    expect(isBackendShortcut("F3")).toBe(true);
    // Sin modificador y sin ser Escape ni una tecla de función: Rust lo rechaza.
    expect(isBackendShortcut("Enter")).toBe(false);
    expect(isBackendShortcut("ArrowDown")).toBe(false);
    expect(isBackendShortcut("Home")).toBe(false);
    expect(isBackendShortcut("Mod+Mod+A")).toBe(false);
  });

  it("impide asignar a navegación una combinación que el backend rechazaría", () => {
    const bindings = resolveShortcuts();
    expect(describeShortcutRejection(bindings, "library", "Mod+9")).toBeUndefined();
    expect(describeShortcutRejection(bindings, "library", "Mod+Comma")).toMatch(/reservado/);
    expect(describeShortcutRejection(bindings, "library", "Mod+2")).toMatch(
      /ya está asignado a Planificador/,
    );
    expect(describeShortcutRejection(bindings, "library", "Insert")).toMatch(/navegación/);
    // La misma tecla sí vale para una acción local, que no viaja a SQLite.
    expect(describeShortcutRejection(bindings, "openDetail", "Insert")).toBeUndefined();
  });
});

describe("resolución del mapa completo", () => {
  it("fusiona navegación y biblioteca con los valores de fábrica", () => {
    const bindings = resolveShortcuts();
    expect(bindings.commandPalette).toBe("Mod+K");
    expect(bindings.search).toBe("Mod+F");
    expect(bindings.primaryAction).toBe("Enter");
    expect(bindings.openDetail).toBe("Space");
    expect(bindings.selectAll).toBe("Mod+A");
  });

  it("migra el Mod+K heredado de la búsqueda para no duplicar la paleta", () => {
    const legacy = { ...DEFAULT_SHORTCUTS, search: LEGACY_SEARCH_SHORTCUT };
    const bindings = resolveShortcuts(legacy);
    expect(bindings.search).toBe("Mod+F");
    expect(bindings.commandPalette).toBe("Mod+K");
  });

  it("cede ante navegación cuando el usuario reasigna encima de una acción local", () => {
    const bindings = resolveShortcuts({ ...DEFAULT_SHORTCUTS, sync: "Mod+K" });
    expect(bindings.sync).toBe("Mod+K");
    // Sin combinación en vez de disparar dos cosas con la misma tecla.
    expect(bindings.commandPalette).toBe("");
    expect(resolveShortcutEvent(keyboardEvent("k", { metaKey: true }), bindings)?.action).toBe(
      "sync",
    );
  });

  it("persiste y recupera las reasignaciones locales", () => {
    writeLocalShortcuts({ openDetail: "Mod+I" });
    expect(readLocalShortcuts()).toEqual({ openDetail: "Mod+I" });
    expect(resolveShortcuts(undefined, readLocalShortcuts()).openDetail).toBe("Mod+I");
    // Las claves desconocidas no entran: el almacenamiento es del usuario.
    window.localStorage.setItem("vindexa:shortcuts:v1", '{"noExiste":"Mod+Z"}');
    expect(readLocalShortcuts()).toEqual({});
  });

  it("resuelve el evento contra la acción correspondiente", () => {
    const bindings = resolveShortcuts();
    expect(resolveShortcutEvent(keyboardEvent("Enter"), bindings)?.action).toBe("primaryAction");
    expect(resolveShortcutEvent(keyboardEvent("k", { metaKey: true }), bindings)?.action).toBe(
      "commandPalette",
    );
    expect(resolveShortcutEvent(keyboardEvent("f", { metaKey: true }), bindings)?.action).toBe(
      "search",
    );
    expect(resolveShortcutEvent(keyboardEvent("z", { metaKey: true }), bindings)?.action).toBe(
      "undo",
    );
    // Una combinación que nadie reclama no resuelve a ninguna acción.
    expect(resolveShortcutEvent(keyboardEvent("y", { metaKey: true }), bindings)).toBeUndefined();
  });
});

describe("catálogo de acciones", () => {
  it("describe cada acción una sola vez y sin combinaciones repetidas", () => {
    const actions = SHORTCUT_CATALOGUE.map((entry) => entry.action);
    expect(new Set(actions).size).toBe(actions.length);
    const defaults = { ...DEFAULT_SHORTCUTS, ...DEFAULT_LOCAL_SHORTCUTS };
    expect(actions.every((action) => action in defaults)).toBe(true);
    const combinations = Object.values(defaults);
    expect(new Set(combinations).size).toBe(combinations.length);
  });

  it("liga las acciones frecuentes a teclas alcanzables", () => {
    const bindings = resolveShortcuts();
    expect(bindings.primaryAction).toBe("Enter");
    expect(bindings.openDetail).toBe("Space");
    expect(bindings.addToCollection).toBe("Mod+Shift+A");
    expect(bindings.addToPlanner).toBe("Mod+Shift+P");
    expect(bindings.search).toBe("Mod+F");
  });

  it("exige modificador para desinstalar", () => {
    // Las flechas mueven el foco por la rejilla; `Supr` a secas quedaría a una
    // tecla de distancia de una acción que abre el desinstalador de Steam.
    const bindings = resolveShortcuts();
    expect(bindings.requestUninstall).toBe("Mod+Delete");
  });

  it("da un salto directo a cada estado, además del ciclo", () => {
    const bindings = resolveShortcuts();
    expect(bindings.statusSlot1).toBe("Alt+1");
    expect(bindings.statusSlot5).toBe("Alt+5");
    expect(bindings.statusForward).toBe("Alt+ArrowRight");
  });

  it("da atajo a todas las secciones, incluida Deseados", () => {
    const bindings = resolveShortcuts();
    expect(sectionShortcut(bindings, "library")).toBe("Mod+1");
    expect(sectionShortcut(bindings, "planner")).toBe("Mod+2");
    expect(sectionShortcut(bindings, "collections")).toBe("Mod+3");
    expect(sectionShortcut(bindings, "tracking")).toBe("Mod+4");
    expect(sectionShortcut(bindings, "wishlist")).toBe("Mod+5");
    // Una sección que se queda sin combinación se declara sin atajo, no rompe.
    // Con dos guardadas compartiendo combinación gana la primera del catálogo,
    // y la otra se queda sin asignar en vez de dejar de responder en silencio.
    expect(
      sectionShortcut(resolveShortcuts({ ...DEFAULT_SHORTCUTS, library: "Mod+5" }), "wishlist"),
    ).toBeUndefined();
  });

  /**
   * Deseados tenía dos dueños.
   *
   * La `struct` de Rust ya guardaba `wishlist: "Mod+5"`, y el frente mantenía
   * además una acción local `gotoWishlist` con la misma combinación. La
   * navegación ganaba el reparto, así que la local se quedaba sin asignar —y la
   * de navegación no estaba en el catálogo, así que **nadie escuchaba ⌘5**:
   * Ajustes decía «sin asignar» y pulsarlo no hacía nada.
   */
  it("⌘5 lleva a Deseados y el catálogo lo reconoce", () => {
    const bindings = resolveShortcuts();
    expect(bindings.wishlist).toBe("Mod+5");
    const descriptor = resolveShortcutEvent(keyboardEvent("5", { metaKey: true }), bindings);
    expect(descriptor?.action).toBe("wishlist");
    expect(descriptor?.scope).toBe("navigation");
    // Y una sola entrada en el catálogo: dos dueños era el fallo.
    expect(SHORTCUT_CATALOGUE.filter((entry) => entry.label === "Deseados")).toHaveLength(1);
  });

  it("reparte las acciones entre los tres ámbitos", () => {
    const scopes = new Set(SHORTCUT_CATALOGUE.map((entry) => entry.scope));
    expect(scopes).toEqual(new Set(["navigation", "library", "game"]));
    expect(SHORTCUT_CATALOGUE.filter((entry) => entry.scope === "game").length).toBeGreaterThan(8);
  });
});

describe("etiquetas de plataforma", () => {
  it("usa los símbolos de Apple en el orden canónico", () => {
    expect(shortcutLabel("Mod+Shift+C", true)).toBe("⇧⌘C");
    expect(shortcutLabel("Mod+K", true)).toBe("⌘K");
    expect(shortcutLabel("Alt+ArrowRight", true)).toBe("⌥→");
    expect(shortcutLabel("Enter", true)).toBe("Intro");
    expect(shortcutLabel("Escape", true)).toBe("Esc");
  });

  it("usa nombres completos fuera de macOS", () => {
    expect(shortcutLabel("Mod+Shift+C", false)).toBe("Ctrl+Mayús+C");
    expect(shortcutLabel("Space", false)).toBe("Espacio");
    expect(shortcutLabel("Delete", false)).toBe("Supr");
  });

  it("traduce el mapa al vocabulario del menú contextual del juego", () => {
    const labels = gameContextShortcuts(resolveShortcuts());
    expect(labels.play).toBe("Enter");
    expect(labels.detail).toBe("Space");
    expect(labels.store).toBe("Mod+T");
    expect(labels.copyAppId).toBe("Mod+Shift+C");
  });
});
