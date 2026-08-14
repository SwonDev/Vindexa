import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppShell } from "@/features/shell/AppShell";
import { DENSITY_METRICS } from "@/features/shell/interface-density";
import { api } from "@/lib/tauri";
import type { AppBootstrap } from "@/lib/types";

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
  beforeEach(() => {
    vi.clearAllMocks();
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
    expect(screen.getByText("Preparando biblioteca local…")).toBeVisible();
    expect(screen.getByRole("navigation", { name: "Secciones principales" })).toBeVisible();
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
    expect(screen.getByText("Steam · sincronización correcta")).toBeVisible();
    firstRender.unmount();

    mockedApi.bootstrap.mockResolvedValueOnce(bootstrapWithSteamSync());
    renderAppShell();
    expect(
      await screen.findByRole("button", {
        name: "Cuenta vinculada · sin sincronizar. Abrir ajustes de Steam",
      }),
    ).toHaveAttribute("data-sync-state", "never");
    expect(screen.getByText("Steam · pendiente de sincronizar")).toBeVisible();
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
    expect(DENSITY_METRICS.comfortable.gridPadding).toBe(28);
    expect(DENSITY_METRICS.comfortable.gridGap).toBe(14);
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

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await waitFor(() => expect(focusSearch).toHaveBeenCalledTimes(1));

    await user.click(screen.getByRole("button", { name: "Abrir ajustes" }));
    expect(await screen.findByRole("heading", { name: "Ajustes de Vindexa" })).toBeVisible();
    fireEvent.keyDown(window, { key: "0", metaKey: true });
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Ajustes de Vindexa" })).not.toBeInTheDocument(),
    );
    window.removeEventListener("vindexa:focus-search", focusSearch);
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
