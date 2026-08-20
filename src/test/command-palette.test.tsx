import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CommandPalette } from "@/features/shell/CommandPalette";
import paletteCss from "@/features/shell/command-palette.css?raw";
import {
  fuzzyScore,
  mergeGameResults,
  normalizeSearchText,
  rankGames,
} from "@/features/shell/command-ranking";
import {
  type LibraryCommand,
  type LibraryContextSnapshot,
  onLibraryCommand,
  resolveShortcuts,
} from "@/features/shell/shortcuts";
import { api } from "@/lib/tauri";
import type { CollectionSummary, GameSummary, PagedGames, StatusDefinition } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    listGames: vi.fn(),
    applyLibraryDrop: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "Vindexa no pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

function game(appId: number, title: string, overrides: Partial<GameSummary> = {}): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 120,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: false,
    statusId: "backlog",
    statusName: "Pendiente",
    statusColor: "#5caac1",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 0,
    drmState: "unknown",
    collectionIds: [],
    genres: [],
    ...overrides,
  };
}

const statuses: StatusDefinition[] = [
  { id: "backlog", name: "Pendiente", color: "#5caac1", position: 0, builtIn: true, gameCount: 3 },
  { id: "playing", name: "Jugando", color: "#a4d007", position: 1, builtIn: true, gameCount: 1 },
];

const collections: CollectionSummary[] = [
  {
    id: "favoritos",
    name: "Favoritos",
    description: "",
    color: "#5caac1",
    icon: "star",
    kind: "manual",
    matchMode: "all",
    position: 0,
    gameCount: 2,
  },
];

const plannerColumns = [
  { id: "playing", name: "Jugando ahora" },
  { id: "later", name: "Más adelante" },
];

const games: GameSummary[] = [
  game(10, "Hollow Knight", { installed: true }),
  game(20, "Hades"),
  game(30, "Celeste"),
  game(40, "Ōkami HD"),
  game(50, "Disco Elysium"),
];

/**
 * Catálogo de títulos reales: es el material con el que se comprueba que el
 * emparejado difuso no devuelve ruido y que las siglas siguen funcionando.
 */
const catalogue: GameSummary[] = [
  game(101, "ELDEN RING"),
  game(102, "Persona 4 Golden"),
  game(103, "Marvel's Spider-Man Remastered"),
  game(104, "The Legend of Zelda: Breath of the Wild"),
  game(105, "FINAL FANTASY 7 Remake"),
  game(106, "Hades"),
  game(107, "El niño y la niña"),
  game(108, "Pokémon Escarlata"),
];

function titles(results: readonly GameSummary[]): string[] {
  return results.map((result) => result.title);
}

function snapshot(overrides: Partial<LibraryContextSnapshot> = {}): LibraryContextSnapshot {
  return {
    games,
    focusedAppId: 10,
    selectedAppIds: [10],
    statuses,
    collections,
    plannerColumns,
    view: "grid",
    scopeLabel: "Todos los juegos",
    ...overrides,
  };
}

const noop = () => undefined;

function paged(items: GameSummary[]): PagedGames {
  return { items, total: items.length, limit: 40, offset: 0 };
}

function renderPalette(props: Partial<Parameters<typeof CommandPalette>[0]> = {}) {
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <CommandPalette
        open
        onOpenChange={onOpenChange}
        bindings={resolveShortcuts()}
        section="library"
        density="compact"
        context={snapshot()}
        onNavigate={noop}
        onOpenSettings={noop}
        onSync={noop}
        onSetDensity={noop}
        onClearArtCache={noop}
        onMaintainArtCache={noop}
        {...props}
      />
    </QueryClientProvider>,
  );
  return { ...result, onOpenChange, queryClient };
}

let commands: LibraryCommand[] = [];
let unsubscribe: () => void = noop;

beforeEach(() => {
  commands = [];
  unsubscribe = onLibraryCommand((command) => commands.push(command));
  mockedApi.listGames.mockReset().mockResolvedValue(paged([]));
  mockedApi.applyLibraryDrop.mockReset().mockResolvedValue({ moved: 0, receipt: undefined });
});

afterEach(() => {
  unsubscribe();
});

function paletteInput(): HTMLElement {
  return screen.getByPlaceholderText("Busca una acción, un juego o una sección…");
}

describe("puntuación difusa", () => {
  it("normaliza tildes y mayúsculas antes de comparar", () => {
    expect(normalizeSearchText("Ōkami HD")).toBe("okami hd");
    expect(normalizeSearchText("Café Noir")).toBe("cafe noir");
  });

  it("prefiere el prefijo, luego el inicio de palabra y luego la subsecuencia", () => {
    const prefix = fuzzyScore("hollow knight", "hollow");
    const wordStart = fuzzyScore("hollow knight", "knight");
    const inner = fuzzyScore("disco elysium", "isco");
    const subsequence = fuzzyScore("hollow knight", "hkt");
    expect(prefix).toBeGreaterThan(wordStart);
    expect(wordStart).toBeGreaterThan(inner);
    expect(inner).toBeGreaterThan(subsequence);
    expect(subsequence).toBeGreaterThanOrEqual(0);
    expect(fuzzyScore("hollow knight", "zzz")).toBe(-1);
  });

  it("ordena los juegos por relevancia y respeta el límite", () => {
    // Con el mismo prefijo gana el título más corto: menos ruido que descartar.
    expect(rankGames(games, "h")[0]?.title).toBe("Hades");
    expect(rankGames(games, "hol")[0]?.title).toBe("Hollow Knight");
    expect(rankGames(games, "okami")[0]?.appId).toBe(40);
    expect(rankGames(games, "de", 2)).toHaveLength(2);
    expect(rankGames(games, "xyzzy")).toEqual([]);
  });

  it("descarta las letras dispersas: «elden» sólo es ELDEN RING", () => {
    expect(titles(rankGames(catalogue, "elden"))).toEqual(["ELDEN RING"]);
    expect(fuzzyScore("persona 4 golden", "elden")).toBe(-1);
    expect(fuzzyScore("marvel's spider-man remastered", "elden")).toBe(-1);
  });

  it("reconoce las siglas de los títulos largos", () => {
    expect(titles(rankGames(catalogue, "p4g"))).toEqual(["Persona 4 Golden"]);
    expect(titles(rankGames(catalogue, "ff7"))).toEqual(["FINAL FANTASY 7 Remake"]);
    expect(titles(rankGames(catalogue, "botw"))).toEqual([
      "The Legend of Zelda: Breath of the Wild",
    ]);
    expect(rankGames(catalogue, "hades")[0]?.title).toBe("Hades");
  });

  it("encuentra igual con tildes y eñes escritas o no", () => {
    expect(titles(rankGames(catalogue, "nino"))).toEqual(["El niño y la niña"]);
    expect(titles(rankGames(catalogue, "niño"))).toEqual(["El niño y la niña"]);
    expect(titles(rankGames(catalogue, "pokemon"))).toEqual(["Pokémon Escarlata"]);
    expect(titles(rankGames(catalogue, "pokémon"))).toEqual(["Pokémon Escarlata"]);
  });

  it("tolera una errata pero no dos huecos sueltos", () => {
    expect(fuzzyScore("hollow knight", "hollw")).toBeGreaterThanOrEqual(0);
    expect(fuzzyScore("hollow knight", "hlwkg")).toBe(-1);
  });

  it("mantiene el filtrado de miles de juegos dentro del presupuesto de un fotograma", () => {
    const inventory = Array.from({ length: 5_000 }, (_, index) =>
      game(100_000 + index, `Juego ${index} de prueba ${index % 97}`),
    );
    const queries = ["j", "ju", "jue", "juego", "prueba", "j4", "de pr", "97", "zzz", "ue b"];
    // Primera pasada: llena la caché de títulos normalizados.
    for (const query of queries) rankGames(inventory, query);
    const started = performance.now();
    for (const query of queries) rankGames(inventory, query);
    const perKeystroke = (performance.now() - started) / queries.length;
    // Un fotograma a 60 Hz son 16,6 ms; el presupuesto real es muy inferior.
    expect(perKeystroke).toBeLessThan(16);
  });
});

describe("mezcla con el catálogo completo", () => {
  it("añade los resultados nuevos detrás y no repite los que ya se veían", () => {
    const local = [catalogue[0] as GameSummary];
    const merged = mergeGameResults(
      local,
      [catalogue[0] as GameSummary, game(200, "Elden Path")],
      "elden",
      4,
    );
    expect(merged.map((result) => result.game.appId)).toEqual([101, 200]);
    expect(merged.map((result) => result.fromCatalog)).toEqual([false, true]);
  });

  it("descarta lo que no coincide con el título y respeta el tope", () => {
    const merged = mergeGameResults([], catalogue, "elden", 4);
    expect(titles(merged.map((result) => result.game))).toEqual(["ELDEN RING"]);
    expect(mergeGameResults([], catalogue, "", 4)).toEqual([]);
  });
});

describe("paleta de comandos", () => {
  it("agrupa acciones del juego enfocado, juegos y secciones", () => {
    renderPalette();
    expect(screen.getByRole("dialog")).toBeVisible();
    expect(screen.getByText("Juego enfocado · Hollow Knight")).toBeVisible();
    expect(screen.getByText("Juegos")).toBeVisible();
    expect(screen.getByText("Ir a")).toBeVisible();
    expect(screen.getByText("Aplicación")).toBeVisible();
    expect(screen.getByRole("option", { name: /Jugar a Hollow Knight/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Abrir la ficha/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Abrir la tienda integrada/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Revelar la carpeta de instalación/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Cambiar el estado a «Jugando»/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Añadir a la colección «Favoritos»/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Copiar el AppID/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Sincronizar con Steam/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Vaciar la caché de arte/ })).toBeVisible();
  });

  it("propone instalar cuando el juego enfocado no está instalado", () => {
    renderPalette({ context: snapshot({ focusedAppId: 20 }) });
    expect(screen.getByRole("option", { name: /Instalar Hades/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /Revelar la carpeta/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Solicitar la desinstalación/ })).toBeNull();
  });

  it("anuncia el atajo de cada entrada con la notación de la plataforma", () => {
    renderPalette();
    const detail = screen.getByRole("option", { name: /Abrir la ficha/ });
    expect(within(detail).getByText("Espacio")).toBeVisible();
    const store = screen.getByRole("option", { name: /Abrir la tienda integrada/ });
    expect(within(store).getByText(/Ctrl\+T|⌘T/)).toBeVisible();
  });

  it("busca con coincidencia difusa entre acciones, juegos y secciones", async () => {
    const user = userEvent.setup();
    renderPalette();
    await user.type(paletteInput(), "okami");
    await waitFor(() => expect(screen.getByRole("option", { name: /Ōkami HD/ })).toBeVisible());
    expect(screen.queryByRole("option", { name: /Hollow Knight/ })).not.toBeInTheDocument();

    await user.clear(paletteInput());
    await user.type(paletteInput(), "colecc");
    await waitFor(() =>
      expect(screen.getByRole("option", { name: /Ir a Colecciones/ })).toBeVisible(),
    );
    expect(screen.getByRole("option", { name: /Añadir a la colección «Favoritos»/ })).toBeVisible();
  });

  it("ofrece un estado vacío útil cuando nada coincide", async () => {
    const user = userEvent.setup();
    renderPalette();
    await user.type(paletteInput(), "qqqzzz");
    await waitFor(() => expect(screen.getByText("Nada coincide con «qqqzzz».")).toBeVisible());
    expect(screen.getByText("Prueba con el título de un juego.")).toBeVisible();
  });

  it("se recorre entera con el teclado y ejecuta la acción principal con Intro", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderPalette();
    const input = paletteInput();
    input.focus();

    const first = screen.getByRole("option", { name: /Jugar a Hollow Knight/ });
    await waitFor(() => expect(first).toHaveAttribute("aria-selected", "true"));

    await user.keyboard("{ArrowDown}");
    await waitFor(() =>
      expect(screen.getByRole("option", { name: /Abrir la ficha/ })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    await user.keyboard("{ArrowUp}");
    await waitFor(() => expect(first).toHaveAttribute("aria-selected", "true"));

    await user.keyboard("{Enter}");
    expect(commands).toEqual([{ kind: "play", appId: 10 }]);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("abre la ficha del juego elegido en los resultados de búsqueda", async () => {
    const user = userEvent.setup();
    renderPalette();
    await user.type(paletteInput(), "celeste");
    const option = await screen.findByRole("option", { name: /Celeste/ });
    await user.click(option);
    expect(commands).toEqual([
      { kind: "focus", appId: 30 },
      { kind: "openDetail", appId: 30 },
    ]);
  });

  it("ejecuta las acciones globales sobre las funciones que recibe", async () => {
    const user = userEvent.setup();
    const onSync = vi.fn();
    const onSetDensity = vi.fn();
    const onNavigate = vi.fn();
    renderPalette({ onSync, onSetDensity, onNavigate });

    await user.click(screen.getByRole("option", { name: /Sincronizar con Steam/ }));
    expect(onSync).toHaveBeenCalledTimes(1);

    renderPalette({ onSync, onSetDensity, onNavigate });
    await user.click(screen.getAllByRole("option", { name: /Usar la densidad cómoda/ })[0]);
    expect(onSetDensity).toHaveBeenCalledWith("comfortable");
  });

  it("no muestra el grupo del juego cuando no hay ninguno enfocado", () => {
    renderPalette({ context: snapshot({ focusedAppId: undefined }) });
    expect(screen.queryByText(/Juego enfocado/)).not.toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Ir a Planificador/ })).toBeVisible();
    expect(screen.getByRole("option", { name: /Ir a Deseados/ })).toBeVisible();
  });
});

describe("búsqueda en el catálogo completo", () => {
  it("mezcla los resultados de SQLite bajo los de la página cargada", async () => {
    const user = userEvent.setup();
    mockedApi.listGames.mockResolvedValue(paged([game(900, "ELDEN RING")]));
    renderPalette();
    await user.type(paletteInput(), "elden");

    const option = await screen.findByRole("option", { name: /ELDEN RING/ });
    expect(within(option).getByText(/Catálogo completo/)).toBeVisible();
    await waitFor(() =>
      expect(mockedApi.listGames).toHaveBeenCalledWith({ query: "elden", limit: 40 }),
    );
  });

  it("no consulta el catálogo con un solo carácter", async () => {
    const user = userEvent.setup();
    renderPalette();
    await user.type(paletteInput(), "h");
    await waitFor(() => expect(screen.getByRole("option", { name: /Hades/ })).toBeVisible());
    expect(mockedApi.listGames).not.toHaveBeenCalled();
  });

  it("se queda con lo local en silencio cuando el catálogo falla", async () => {
    const user = userEvent.setup();
    mockedApi.listGames.mockRejectedValue(new Error("sin Tauri"));
    renderPalette();
    await user.type(paletteInput(), "celeste");

    const option = await screen.findByRole("option", { name: /Celeste/ });
    expect(option).toBeVisible();
    await waitFor(() => expect(mockedApi.listGames).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeVisible();
  });
});

describe("acciones sobre la selección múltiple", () => {
  const selection = snapshot({ selectedAppIds: [10, 20, 30] });

  it("sustituye el grupo del juego enfocado por el de la selección", () => {
    renderPalette({ context: selection });
    expect(screen.queryByText(/Juego enfocado/)).not.toBeInTheDocument();
    expect(screen.getByText("3 juegos seleccionados")).toBeVisible();
    expect(screen.getByRole("option", { name: "Mover 3 juegos a «Jugando»" })).toBeVisible();
    expect(
      screen.getByRole("option", { name: "Añadir 3 juegos a la colección «Favoritos»" }),
    ).toBeVisible();
    expect(screen.getByRole("option", { name: "Fijar 3 juegos en la biblioteca" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Marcar seguimiento en 3 juegos" })).toBeVisible();
    expect(
      screen.getByRole("option", { name: "Añadir 3 juegos al plan · Más adelante" }),
    ).toBeVisible();
  });

  it("cuenta sólo los juegos que van a cambiar al fijar una selección mixta", () => {
    renderPalette({
      context: snapshot({
        selectedAppIds: [10, 20, 30],
        games: [
          game(10, "Hollow Knight", { pinned: true }),
          game(20, "Hades"),
          game(30, "Celeste"),
        ],
      }),
    });
    expect(screen.getByRole("option", { name: "Fijar 2 juegos en la biblioteca" })).toBeVisible();
  });

  it("mueve toda la selección de estado en una sola llamada", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderPalette({ context: selection });
    await user.click(screen.getByRole("option", { name: "Mover 3 juegos a «Jugando»" }));
    await waitFor(() =>
      expect(mockedApi.applyLibraryDrop).toHaveBeenCalledWith({
        appIds: [10, 20, 30],
        target: { kind: "status", id: "playing" },
      }),
    );
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("deja el error a la vista y no cierra la paleta", async () => {
    const user = userEvent.setup();
    mockedApi.applyLibraryDrop.mockRejectedValue(
      new Error("La columna ha alcanzado su límite de trabajo en curso."),
    );
    const { onOpenChange } = renderPalette({ context: selection });
    await user.click(screen.getByRole("option", { name: "Mover 3 juegos a «Jugando»" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "La columna ha alcanzado su límite de trabajo en curso.",
      ),
    );
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("fija la selección con una orden por juego pendiente", async () => {
    const user = userEvent.setup();
    renderPalette({ context: selection });
    await user.click(screen.getByRole("option", { name: "Fijar 3 juegos en la biblioteca" }));
    expect(commands).toEqual([
      { kind: "togglePinned", appId: 10 },
      { kind: "togglePinned", appId: 20 },
      { kind: "togglePinned", appId: 30 },
    ]);
  });
});

describe("añadir al plan", () => {
  it("ofrece una entrada por columna publicada por la biblioteca", () => {
    renderPalette();
    expect(screen.getByRole("option", { name: "Añadir al plan · Jugando ahora" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Añadir al plan · Más adelante" })).toBeVisible();
  });

  it("no ofrece el plan mientras la biblioteca no publique sus columnas", () => {
    renderPalette({ context: snapshot({ plannerColumns: undefined }) });
    expect(screen.queryByRole("option", { name: /Añadir al plan/ })).not.toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Abrir la ficha/ })).toBeVisible();
  });

  it("planifica el juego enfocado en la columna elegida", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderPalette();
    await user.click(screen.getByRole("option", { name: "Añadir al plan · Más adelante" }));
    expect(commands).toEqual([{ kind: "addToPlanner", appIds: [10], columnId: "later" }]);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("planifica toda la selección de una vez", async () => {
    const user = userEvent.setup();
    renderPalette({ context: snapshot({ selectedAppIds: [10, 20, 30] }) });
    await user.click(
      screen.getByRole("option", { name: "Añadir 3 juegos al plan · Jugando ahora" }),
    );
    expect(commands).toEqual([{ kind: "addToPlanner", appIds: [10, 20, 30], columnId: "playing" }]);
  });
});

describe("contrato de diseño de la paleta", () => {
  it("no dibuja ni un círculo: todos los radios caben en 0–4 px", () => {
    const radii = [...paletteCss.matchAll(/border-radius:\s*([^;]+);/g)].map((match) =>
      (match[1] as string).trim(),
    );
    expect(radii.length).toBeGreaterThan(0);
    for (const radius of radii) {
      expect(radius).not.toMatch(/9999|999px|50%/);
      for (const value of radius.match(/(\d+(?:\.\d+)?)px/g) ?? []) {
        expect(Number.parseFloat(value)).toBeLessThanOrEqual(4);
      }
    }
  });

  it("todo el color sale de tokens: no hay ni un literal hexadecimal", () => {
    expect(paletteCss).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
  });

  it("sólo usa tokens de texto con AA comprobado", () => {
    const textTokens = new Set(["--foreground", "--v-muted", "--v-subtle", "--v-error"]);
    const declared = [...paletteCss.matchAll(/(?<![-\w])color:\s*var\((--[\w-]+)\)/g)].map(
      (match) => match[1] as string,
    );
    expect(declared.length).toBeGreaterThan(0);
    for (const token of declared) {
      if (token === "inherit") continue;
      expect(textTokens.has(token), `token de texto no permitido: ${token}`).toBe(true);
    }
  });

  it("mueve sólo transform y opacidad, entre 120 y 260 ms y con la curva del sistema", () => {
    const durations = [...paletteCss.matchAll(/(\d+)ms/g)].map((match) =>
      Number.parseInt(match[1] as string, 10),
    );
    expect(durations.length).toBeGreaterThan(0);
    for (const duration of durations) {
      expect(duration).toBeGreaterThanOrEqual(120);
      expect(duration).toBeLessThanOrEqual(260);
    }
    const animated = [...paletteCss.matchAll(/^\s{4}(\w[\w-]*) (\d+)ms/gm)].map(
      (match) => match[1] as string,
    );
    for (const property of animated) {
      expect(["transform", "opacity"]).toContain(property);
    }
    expect(paletteCss).not.toMatch(/cubic-bezier/);
    expect(paletteCss).toContain("var(--ease-out)");
  });

  it("anula el movimiento cuando el sistema lo pide", () => {
    expect(paletteCss).toContain("@media (prefers-reduced-motion: reduce)");
    expect(paletteCss).toMatch(/animation:\s*none;/);
    expect(paletteCss).toMatch(/transition:\s*none;/);
    // Sin `!important`: la hoja gana por estar fuera de las capas de Tailwind.
    expect(paletteCss).not.toContain("!important");
  });
});
