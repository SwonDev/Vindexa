import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppShell } from "@/features/shell/AppShell";
import { DENSITY_METRICS } from "@/features/shell/interface-density";
import {
  type LibraryCommand,
  onLibraryCommand,
  publishLibraryContext,
  writeLocalShortcuts,
} from "@/features/shell/shortcuts";
import { api } from "@/lib/tauri";
import type { AppBootstrap, GameSummary } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    bootstrap: vi.fn(),
    listGames: vi.fn(),
    importLocalSteam: vi.fn(),
    libraryFilterOptions: vi.fn(),
    metadataEnrichmentStatus: vi.fn(),
    startMetadataEnrichment: vi.fn(),
    syncSteamLibrary: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as {
  bootstrap: ReturnType<typeof vi.fn>;
  listGames: ReturnType<typeof vi.fn>;
  importLocalSteam: ReturnType<typeof vi.fn>;
  syncSteamLibrary: ReturnType<typeof vi.fn>;
  libraryFilterOptions: ReturnType<typeof vi.fn>;
  metadataEnrichmentStatus: ReturnType<typeof vi.fn>;
  startMetadataEnrichment: ReturnType<typeof vi.fn>;
};

const emptyBootstrap: AppBootstrap = {
  stats: {
    totalGames: 0,
    installedGames: 0,
    playingGames: 0,
    backlogGames: 0,
    trackedGames: 0,
    totalPlaytimeMinutes: 0,
  },
  statuses: [],
  collections: [],
  planner: [],
  steam: {
    apiKeyConfigured: false,
    apiKeyVerificationRequired: false,
    localSteamDetected: true,
    localManifestCount: 2,
  },
  preferences: {
    density: "compact",
    periodicSyncMinutes: 0,
    confirmUninstall: true,
    librarySort: "manual",
    shortcuts: {
      library: "Mod+1",
      planner: "Mod+2",
      collections: "Mod+3",
      tracking: "Mod+4",
      search: "Mod+K",
      sync: "Mod+Shift+S",
      closePanel: "Escape",
    },
  },
  databasePath: "/Users/prueba/Library/Application Support/Vindexa/vindexa.sqlite3",
};

function bootstrapWithSteamSync(
  status?: "success" | "failed",
  lastSyncErrorMessage?: string,
): AppBootstrap {
  return {
    ...emptyBootstrap,
    steam: {
      ...emptyBootstrap.steam,
      apiKeyConfigured: true,
      account: {
        steamId: "76561198000000000",
        personaName: "Vindexa QA",
        ...(status ? { lastSyncStatus: status } : {}),
        ...(lastSyncErrorMessage ? { lastSyncErrorMessage } : {}),
      },
    },
  };
}

const focusedGame: GameSummary = {
  appId: 730,
  title: "Counter-Strike 2",
  playtimeMinutes: 4_200,
  playtimeRecentMinutes: 0,
  isEarlyAccess: false,
  isFree: true,
  ownershipSource: "owned",
  familyAvailability: "not_applicable",
  installed: true,
  statusId: "playing",
  statusName: "Jugando",
  statusColor: "#a4d007",
  progress: 40,
  priority: 2,
  pinned: false,
  tracking: false,
  manualPosition: 0,
  collectionIds: [],
  genres: [],
};

function libraryContext() {
  return {
    games: [focusedGame],
    focusedAppId: focusedGame.appId,
    selectedAppIds: [focusedGame.appId],
    statuses: [],
    collections: [],
    view: "grid" as const,
    scopeLabel: "Todos los juegos",
  };
}

/** La rejilla real lleva esta clase; los atajos desnudos sólo actúan dentro. */
function librarySurface(): HTMLElement {
  const node = document.createElement("div");
  node.className = "game-browser";
  document.body.append(node);
  return node;
}

function renderAppShell() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
      mutations: { retry: false },
    },
  });
  const renderResult = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <AppShell />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return { ...renderResult, queryClient };
}

describe("ciclo de carga de la aplicación", () => {
  afterEach(() => {
    window.localStorage.clear();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    mockedApi.syncSteamLibrary.mockResolvedValue(undefined);
    mockedApi.libraryFilterOptions.mockResolvedValue({
      genres: [],
      categories: [],
      tags: [],
      totalGames: 0,
      metadataGames: 0,
      achievementGames: 0,
      steamDeckGames: 0,
    });
    mockedApi.metadataEnrichmentStatus.mockResolvedValue({
      running: false,
      totalGames: 0,
      freshMetadata: 0,
      queued: 0,
      processing: 0,
      retrying: 0,
      succeeded: 0,
      unavailable: 0,
      failed: 0,
      steamDeckAvailability: "disabled",
      steamDeckExplanation: "Sin una fuente documentada.",
    });
  });

  it("mantiene geometría estable mientras SQLite prepara la biblioteca", () => {
    mockedApi.bootstrap.mockImplementation(() => new Promise(() => undefined));
    mockedApi.listGames.mockImplementation(() => new Promise(() => undefined));

    renderAppShell();

    expect(screen.getByRole("status", { name: "Cargando juegos" })).toBeVisible();
    expect(screen.getByRole("navigation", { name: "Secciones principales" })).toBeVisible();
    // La barra de estado no dice «preparando»: que la base esté abriéndose ya
    // lo cuenta la propia pantalla, y repetirlo sólo gastaba una fila de alto.
    expect(screen.queryByText(/Preparando biblioteca local/)).toBeNull();
  });

  it("presenta un error recuperable cuando no puede abrir la base local", async () => {
    mockedApi.bootstrap.mockRejectedValue(new Error("SQLite no pudo abrir el archivo local."));
    mockedApi.listGames.mockImplementation(() => new Promise(() => undefined));

    renderAppShell();

    expect(
      await screen.findByRole("heading", { name: "No se pudo abrir la biblioteca" }),
    ).toBeVisible();
    expect(screen.getByText("SQLite no pudo abrir el archivo local.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Reintentar" })).toBeEnabled();
  });

  it("ofrece importar datos reales en primera ejecución y comunica el resultado", async () => {
    const user = userEvent.setup();
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    mockedApi.importLocalSteam.mockResolvedValue({
      steamPath: "/Users/prueba/Library/Application Support/Steam",
      librariesScanned: 1,
      importedGames: 2,
      updatedGames: 0,
    });

    renderAppShell();

    expect(
      await screen.findByRole("heading", { name: "Construye tu biblioteca real" }),
    ).toBeVisible();
    expect(screen.getByText(/Importa los manifiestos instalados/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Importar Steam local" }));

    expect(mockedApi.importLocalSteam).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("2 juegos locales importados.")).toHaveAttribute(
      "role",
      "status",
    );
  });

  it("presenta el fallo de sincronización como acción de recuperación, no como salud verde", async () => {
    const user = userEvent.setup();
    mockedApi.bootstrap.mockResolvedValue(
      bootstrapWithSteamSync("failed", "Steam no permite leer esta biblioteca privada."),
    );
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });

    renderAppShell();

    const steamAction = await screen.findByRole("button", {
      name: "Cuenta vinculada · sincronización fallida. Abrir ajustes de Steam",
    });
    expect(steamAction).toHaveAttribute("data-sync-state", "failed");
    expect(steamAction).toHaveTextContent("Steam · sync fallida");
    expect(screen.getByText("Steam · sincronización fallida")).toBeVisible();

    await user.click(steamAction);
    expect(await screen.findByRole("heading", { name: "Ajustes de Vindexa" })).toBeVisible();
    expect(screen.getByText("Steam no permite leer esta biblioteca privada.")).toBeVisible();
  });

  it("distingue una sincronización correcta de una cuenta aún no sincronizada", async () => {
    mockedApi.bootstrap.mockResolvedValueOnce(bootstrapWithSteamSync("success"));
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });

    const firstRender = renderAppShell();
    expect(
      await screen.findByRole("button", {
        name: "Cuenta vinculada · sincronizada. Abrir ajustes de Steam",
      }),
    ).toHaveAttribute("data-sync-state", "success");
    // Una sincronización correcta no se anuncia dos veces: lo dice la ficha de
    // la cabecera y la barra de estado se queda callada.
    expect(screen.queryByText("Steam · sincronización correcta")).toBeNull();
    firstRender.unmount();

    mockedApi.bootstrap.mockResolvedValueOnce(bootstrapWithSteamSync());
    renderAppShell();
    expect(
      await screen.findByRole("button", {
        name: "Cuenta vinculada · sin sincronizar. Abrir ajustes de Steam",
      }),
    ).toHaveAttribute("data-sync-state", "never");
    expect(screen.queryByText("Steam · pendiente de sincronizar")).toBeNull();
  });

  it("la barra de estado sólo aparece cuando hay algo que no se ve en otro sitio", async () => {
    mockedApi.bootstrap.mockResolvedValue(bootstrapWithSteamSync("failed"));
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    mockedApi.metadataEnrichmentStatus.mockResolvedValue({
      running: true,
      totalGames: 2294,
      freshMetadata: 1947,
      queued: 347,
      processing: 4,
      retrying: 0,
      succeeded: 1947,
      unavailable: 0,
      failed: 12,
      steamDeckAvailability: "disabled",
      steamDeckExplanation: "Sin una fuente documentada.",
    });

    renderAppShell();

    // Trabajo en curso, con lo que falta —no con lo ya hecho, que no se puede
    // accionar—; los fallos, para poder mirarlos; y el detalle de una
    // sincronización rota, que el punto de color de la cabecera no cabe a
    // explicar.
    expect(await screen.findByText(/Completando fichas · faltan 347/)).toBeVisible();
    expect(screen.getByText(/12 fichas no se han podido leer/)).toBeVisible();
    expect(screen.getByText("Steam · sincronización fallida")).toBeVisible();
  });

  it("aplica la densidad persistida y mantiene métricas virtuales diferenciadas", async () => {
    mockedApi.bootstrap.mockResolvedValue({
      ...emptyBootstrap,
      preferences: { ...emptyBootstrap.preferences, density: "comfortable" },
    });
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });

    renderAppShell();

    expect(
      await screen.findByRole("heading", { name: "Construye tu biblioteca real" }),
    ).toBeVisible();
    expect(document.querySelector(".app-shell")).toHaveAttribute("data-density", "comfortable");
    expect(DENSITY_METRICS.comfortable.listRow).toBeGreaterThan(DENSITY_METRICS.compact.listRow);
    expect(DENSITY_METRICS.comfortable.gridBody).toBeGreaterThan(DENSITY_METRICS.compact.gridBody);
    expect(DENSITY_METRICS.comfortable.gridPadding).toBeGreaterThan(
      DENSITY_METRICS.compact.gridPadding,
    );
    expect(DENSITY_METRICS.comfortable.gridGap).toBeGreaterThan(DENSITY_METRICS.compact.gridGap);
  });

  it("aplica atajos persistidos y no intercepta escritura en campos", async () => {
    mockedApi.bootstrap.mockResolvedValue({
      ...emptyBootstrap,
      preferences: {
        ...emptyBootstrap.preferences,
        shortcuts: { ...emptyBootstrap.preferences.shortcuts, planner: "Mod+K" },
      },
    });
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });
    const library = await screen.findByRole("button", { name: "Biblioteca" });
    const planner = screen.getByRole("button", { name: "Planificador" });
    const input = document.createElement("input");
    document.body.append(input);

    fireEvent.keyDown(input, { key: "k", metaKey: true });
    expect(library).toHaveAttribute("aria-current", "page");
    expect(planner).not.toHaveAttribute("aria-current");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await waitFor(() => expect(planner).toHaveAttribute("aria-current", "page"));
    input.remove();
  });

  it("el atajo de sincronización ejecuta la operación real y anuncia el resultado", async () => {
    mockedApi.bootstrap.mockResolvedValue(bootstrapWithSteamSync("success"));
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    mockedApi.syncSteamLibrary.mockResolvedValue({
      steamId: "76561198000000000",
      importedGames: 0,
      updatedGames: 2,
      privateLibrarySuspected: false,
      familyMembersDetected: 0,
      familyMembersInaccessible: 0,
      familyGamesImported: 0,
      familyCatalogGamesDetected: 0,
      completedAt: "2026-08-14T19:00:00Z",
    });
    const { queryClient } = renderAppShell();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await screen.findByRole("button", {
      name: "Cuenta vinculada · sincronizada. Abrir ajustes de Steam",
    });

    fireEvent.keyDown(window, { key: "s", metaKey: true, shiftKey: true });
    await waitFor(() => expect(mockedApi.syncSteamLibrary).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Sincronización manual completada.")).toBeInTheDocument();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["family-catalog"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library-filter-options"] });
  });

  it("evita sincronizaciones concurrentes al repetir el atajo", async () => {
    mockedApi.bootstrap.mockResolvedValue(bootstrapWithSteamSync("success"));
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    mockedApi.syncSteamLibrary.mockReturnValue(new Promise(() => undefined));
    renderAppShell();
    await screen.findByRole("button", {
      name: "Cuenta vinculada · sincronizada. Abrir ajustes de Steam",
    });

    fireEvent.keyDown(window, { key: "s", metaKey: true, shiftKey: true });
    fireEvent.keyDown(window, { key: "s", metaKey: true, shiftKey: true });

    expect(mockedApi.syncSteamLibrary).toHaveBeenCalledTimes(1);
  });

  it("dirige la búsqueda y cierra el panel activo con sus combinaciones configuradas", async () => {
    mockedApi.bootstrap.mockResolvedValue({
      ...emptyBootstrap,
      preferences: {
        ...emptyBootstrap.preferences,
        shortcuts: { ...emptyBootstrap.preferences.shortcuts, closePanel: "Mod+0" },
      },
    });
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    const focusSearch = vi.fn();
    window.addEventListener("vindexa:focus-search", focusSearch);
    const user = userEvent.setup();
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    // `Mod+K` ya es la paleta: la búsqueda vive en `Mod+F`.
    fireEvent.keyDown(window, { key: "f", metaKey: true });
    await waitFor(() => expect(focusSearch).toHaveBeenCalledTimes(1));

    await user.click(screen.getByRole("button", { name: "Abrir ajustes" }));
    expect(await screen.findByRole("heading", { name: "Ajustes de Vindexa" })).toBeVisible();
    fireEvent.keyDown(window, { key: "0", metaKey: true });
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Ajustes de Vindexa" })).not.toBeInTheDocument(),
    );
    window.removeEventListener("vindexa:focus-search", focusSearch);
  });

  it("navega a Deseados con su atajo local, sin tocar el esquema de SQLite", async () => {
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });
    const library = screen.getByRole("button", { name: "Biblioteca" });

    fireEvent.keyDown(window, { key: "5", metaKey: true });

    await waitFor(() => expect(library).not.toHaveAttribute("aria-current"));
    expect(mockedApi.savePreferences).toBeUndefined();
  });

  it("abre la paleta de comandos con Mod+K y la cierra con Escape", async () => {
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByPlaceholderText("Busca una acción, un juego o una sección…"),
    ).toBeVisible();

    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("también abre la paleta desde la barra superior", async () => {
    const user = userEvent.setup();
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    await user.click(screen.getByRole("button", { name: "Abrir la paleta de comandos" }));
    expect(await screen.findByRole("dialog")).toBeVisible();
  });

  it("ejecuta la acción principal sobre el juego enfocado y mueve el foco con las flechas", async () => {
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    const commands: LibraryCommand[] = [];
    const stop = onLibraryCommand((command) => commands.push(command));
    const surface = librarySurface();
    publishLibraryContext(libraryContext());

    fireEvent.keyDown(surface, { key: "Enter" });
    fireEvent.keyDown(surface, { key: " " });
    fireEvent.keyDown(surface, { key: "ArrowDown" });
    fireEvent.keyDown(surface, { key: "ArrowRight", shiftKey: true });
    fireEvent.keyDown(surface, { key: "Home" });
    fireEvent.keyDown(surface, { key: "a", metaKey: true });
    // `LibraryScreen` publica el contexto real en cada renderizado, y la
    // biblioteca de esta prueba está vacía: hay que volver a declarar el foco
    // sintético antes de las teclas que operan sobre un juego concreto.
    publishLibraryContext(libraryContext());
    fireEvent.keyDown(surface, { key: "d", metaKey: true });
    publishLibraryContext(libraryContext());
    fireEvent.keyDown(surface, { key: "ArrowRight", altKey: true });

    expect(commands).toEqual([
      { kind: "primary", appId: 730 },
      { kind: "openDetail", appId: 730 },
      { kind: "moveFocus", direction: "down", extend: false },
      { kind: "moveFocus", direction: "right", extend: true },
      { kind: "moveFocus", direction: "first", extend: false },
      { kind: "selectAll" },
      { kind: "togglePinned", appId: 730 },
      { kind: "cycleStatus", appId: 730, direction: 1 },
    ]);
    stop();
    surface.remove();
  });

  it("no roba Intro ni las flechas fuera de la rejilla de la biblioteca", async () => {
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    const commands: LibraryCommand[] = [];
    const stop = onLibraryCommand((command) => commands.push(command));
    publishLibraryContext(libraryContext());
    const outside = document.createElement("button");
    document.body.append(outside);

    fireEvent.keyDown(outside, { key: "Enter" });
    fireEvent.keyDown(outside, { key: "ArrowDown" });

    expect(commands).toEqual([]);
    stop();
    outside.remove();
  });

  it("no dispara ningún atajo mientras se escribe una nota", async () => {
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    const commands: LibraryCommand[] = [];
    const stop = onLibraryCommand((command) => commands.push(command));
    publishLibraryContext(libraryContext());
    const surface = librarySurface();
    const notes = document.createElement("textarea");
    surface.append(notes);

    fireEvent.keyDown(notes, { key: "Enter" });
    fireEvent.keyDown(notes, { key: "ArrowDown" });
    fireEvent.keyDown(notes, { key: "k", metaKey: true });
    fireEvent.keyDown(notes, { key: "d", metaKey: true });

    expect(commands).toEqual([]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    stop();
    surface.remove();
  });

  it("aplica una reasignación local sin perder los atajos de navegación", async () => {
    writeLocalShortcuts({ togglePinned: "Mod+Shift+P" });
    mockedApi.bootstrap.mockResolvedValue(emptyBootstrap);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });

    const commands: LibraryCommand[] = [];
    const stop = onLibraryCommand((command) => commands.push(command));
    publishLibraryContext(libraryContext());
    const surface = librarySurface();

    fireEvent.keyDown(surface, { key: "d", metaKey: true });
    fireEvent.keyDown(surface, { key: "P", metaKey: true, shiftKey: true });
    expect(commands).toEqual([{ kind: "togglePinned", appId: 730 }]);

    const planner = screen.getByRole("button", { name: "Planificador" });
    fireEvent.keyDown(window, { key: "2", metaKey: true });
    await waitFor(() => expect(planner).toHaveAttribute("aria-current", "page"));
    stop();
    surface.remove();
  });

  it("cede la combinación a navegación cuando el usuario la reasigna encima de una local", async () => {
    mockedApi.bootstrap.mockResolvedValue({
      ...emptyBootstrap,
      preferences: {
        ...emptyBootstrap.preferences,
        shortcuts: { ...emptyBootstrap.preferences.shortcuts, collections: "Mod+K" },
      },
    });
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    renderAppShell();
    await screen.findByRole("heading", { name: "Construye tu biblioteca real" });
    const collections = screen.getByRole("button", { name: "Colecciones" });

    fireEvent.keyDown(window, { key: "k", metaKey: true });

    await waitFor(() => expect(collections).toHaveAttribute("aria-current", "page"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("anuncia un fallo periódico e invalida cuenta y juegos para reflejarlo", async () => {
    vi.useFakeTimers();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      mockedApi.bootstrap.mockResolvedValue({
        ...bootstrapWithSteamSync("success"),
        preferences: { ...emptyBootstrap.preferences, periodicSyncMinutes: 1 },
      });
      mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
      mockedApi.syncSteamLibrary.mockRejectedValueOnce(
        new Error("Steam temporalmente no disponible."),
      );

      const { queryClient } = renderAppShell();
      const invalidate = vi.spyOn(queryClient, "invalidateQueries");
      await act(async () => vi.advanceTimersByTimeAsync(0));
      await act(async () => vi.advanceTimersByTimeAsync(60_000));

      expect(mockedApi.syncSteamLibrary).toHaveBeenCalledTimes(1);
      expect(
        screen.getByText("Sincronización periódica fallida: Steam temporalmente no disponible."),
      ).toHaveAttribute("role", "status");
      expect(mockedApi.bootstrap.mock.calls.length).toBeGreaterThanOrEqual(2);
      expect(mockedApi.listGames.mock.calls.length).toBeGreaterThanOrEqual(2);
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["family-catalog"] });
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library-filter-options"] });
    } finally {
      consoleError.mockRestore();
      vi.useRealTimers();
    }
  });
});
