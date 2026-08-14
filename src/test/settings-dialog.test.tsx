import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SettingsDialog } from "@/features/settings/SettingsDialog";
import { api } from "@/lib/tauri";
import type { AppBootstrap } from "@/lib/types";

vi.mock("@/lib/tauri", () => {
  const mockedApi = {
    startSteamLogin: vi.fn(),
    syncSteamLibrary: vi.fn(),
    importLocalSteam: vi.fn(),
    saveSteamApiKey: vi.fn(),
    deleteSteamApiKey: vi.fn(),
    verifySavedSteamApiKey: vi.fn(),
    unlinkSteam: vi.fn(),
    savePreferences: vi.fn(),
    diagnostics: vi.fn(),
    exportBackup: vi.fn(),
    importBackup: vi.fn(),
    clearArtCache: vi.fn(),
    checkForUpdates: vi.fn(),
    deleteStatus: vi.fn(),
    deletePlannerColumn: vi.fn(),
  };
  return {
    api: mockedApi,
    getErrorMessage: (error: unknown) =>
      error instanceof Error ? error.message : "No se pudo completar la operación.",
  };
});

const mockedApi = api as unknown as {
  [Key in keyof typeof api]: ReturnType<typeof vi.fn>;
};

const bootstrap: AppBootstrap = {
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
    periodicSyncMinutes: 60,
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

function renderSettings(initialBootstrap = bootstrap) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const renderResult = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <SettingsDialog open onOpenChange={vi.fn()} bootstrap={initialBootstrap} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return { ...renderResult, queryClient };
}

describe("ajustes y secretos", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.saveSteamApiKey.mockResolvedValue(undefined);
    mockedApi.verifySavedSteamApiKey.mockResolvedValue(false);
    mockedApi.syncSteamLibrary.mockResolvedValue({
      steamId: "76561198000000000",
      importedGames: 12,
      updatedGames: 4,
      privateLibrarySuspected: false,
      familyMembersDetected: 0,
      familyMembersInaccessible: 0,
      familyGamesImported: 0,
      familyCatalogGamesDetected: 0,
      completedAt: "2026-08-14T18:00:00Z",
    });
    mockedApi.savePreferences.mockResolvedValue(undefined);
    mockedApi.deleteSteamApiKey.mockResolvedValue(undefined);
    mockedApi.deleteStatus.mockResolvedValue(undefined);
    mockedApi.deletePlannerColumn.mockResolvedValue(undefined);
    mockedApi.importBackup.mockResolvedValue(false);
    mockedApi.diagnostics.mockResolvedValue({
      path: bootstrap.databasePath,
      sizeBytes: 4_096,
      schemaVersion: 2,
      integrity: "ok",
      walEnabled: true,
    });
  });

  it("mantiene la API key oculta, permite revelarla y valida antes de persistir", async () => {
    const user = userEvent.setup();
    renderSettings();

    const apiKey = screen.getByLabelText("Web API Key de Steam");
    expect(apiKey).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Mostrar clave" }));
    expect(apiKey).toHaveAttribute("type", "text");
    expect(screen.getByRole("button", { name: "Ocultar clave" })).toBeVisible();

    await user.type(apiKey, "corta");
    await user.click(screen.getByRole("button", { name: /Guardar de forma segura/ }));
    expect(await screen.findByText("Introduce una Web API Key válida.")).toBeVisible();
    expect(mockedApi.saveSteamApiKey).not.toHaveBeenCalled();

    await user.clear(apiKey);
    await user.type(apiKey, "0123456789abcdef0123456789abcdef");
    await user.click(screen.getByRole("button", { name: /Guardar de forma segura/ }));
    await waitFor(() =>
      expect(mockedApi.saveSteamApiKey).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef"),
    );
    expect(
      await screen.findByText("La Web API Key se guardó en el almacén seguro del sistema."),
    ).toBeVisible();
    expect(apiKey).toHaveValue("");
    expect(apiKey).toHaveAttribute("type", "password");
    expect(mockedApi.syncSteamLibrary).not.toHaveBeenCalled();
  });

  it("solo consulta Keychain cuando la persona solicita comprobar la clave", async () => {
    const user = userEvent.setup();
    mockedApi.verifySavedSteamApiKey.mockResolvedValueOnce(true);
    renderSettings({
      ...bootstrap,
      steam: { ...bootstrap.steam, apiKeyVerificationRequired: true },
    });

    expect(mockedApi.verifySavedSteamApiKey).not.toHaveBeenCalled();
    expect(screen.getByText(/Vindexa no consulta Keychain al iniciar/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Comprobar clave guardada" }));

    expect(mockedApi.verifySavedSteamApiKey).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText("La clave guardada está disponible para sincronizar con Steam."),
    ).toBeVisible();
  });

  it("recupera el último error persistente de sincronización al abrir", () => {
    renderSettings({
      ...bootstrap,
      steam: {
        ...bootstrap.steam,
        account: {
          steamId: "76561198000000000",
          lastSyncErrorCode: "steam_library_private",
          lastSyncErrorMessage: "Steam no permite leer esta biblioteca privada.",
        },
      },
    });

    expect(screen.getByText("Steam no permite leer esta biblioteca privada.")).toBeVisible();
  });

  it("guarda y sincroniza una sola vez cuando la cuenta ya está vinculada", async () => {
    const user = userEvent.setup();
    const linkedBootstrap: AppBootstrap = {
      ...bootstrap,
      steam: {
        ...bootstrap.steam,
        account: {
          steamId: "76561198000000000",
          personaName: "Vindexa QA",
        },
      },
    };
    const { queryClient } = renderSettings(linkedBootstrap);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    await user.type(
      screen.getByLabelText("Web API Key de Steam"),
      "0123456789abcdef0123456789abcdef",
    );
    await user.click(screen.getByRole("button", { name: /Guardar y sincronizar/ }));

    await waitFor(() => expect(mockedApi.syncSteamLibrary).toHaveBeenCalledTimes(1));
    expect(mockedApi.saveSteamApiKey).toHaveBeenCalledTimes(1);
    expect(mockedApi.saveSteamApiKey.mock.invocationCallOrder[0]).toBeLessThan(
      mockedApi.syncSteamLibrary.mock.invocationCallOrder[0] ?? 0,
    );
    expect(
      await screen.findByText(
        "Clave guardada. Sincronización completada: 12 importados y 4 actualizados.",
      ),
    ).toBeVisible();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["bootstrap"] });
  });

  it("muestra sin alterar el error de sincronización posterior al guardado", async () => {
    const user = userEvent.setup();
    mockedApi.syncSteamLibrary.mockRejectedValueOnce(
      new Error("Steam devolvió una biblioteca privada."),
    );
    const linkedBootstrap: AppBootstrap = {
      ...bootstrap,
      steam: {
        ...bootstrap.steam,
        account: { steamId: "76561198000000000" },
      },
    };
    const { queryClient } = renderSettings(linkedBootstrap);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    await user.type(
      screen.getByLabelText("Web API Key de Steam"),
      "0123456789abcdef0123456789abcdef",
    );
    await user.click(screen.getByRole("button", { name: /Guardar y sincronizar/ }));

    expect(await screen.findByText("Steam devolvió una biblioteca privada.")).toBeVisible();
    expect(screen.getByLabelText("Web API Key de Steam")).toHaveValue("");
    expect(screen.getByLabelText("Web API Key de Steam")).toHaveAttribute("type", "password");
    expect(mockedApi.saveSteamApiKey).toHaveBeenCalledTimes(1);
    expect(mockedApi.syncSteamLibrary).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["bootstrap"] });
  });

  it("exige confirmación antes de eliminar la Web API Key", async () => {
    const user = userEvent.setup();
    renderSettings({
      ...bootstrap,
      steam: { ...bootstrap.steam, apiKeyConfigured: true },
    });

    await user.click(screen.getByRole("button", { name: "Eliminar clave" }));
    let dialog = screen.getByRole("alertdialog");
    expect(within(dialog).getByText("¿Eliminar la Web API Key?")).toBeVisible();
    expect(within(dialog).getByText(/cuenta vinculada, biblioteca y datos locales/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancelar" }));
    expect(mockedApi.deleteSteamApiKey).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Eliminar clave" }));
    dialog = screen.getByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Eliminar clave" }));
    await waitFor(() => expect(mockedApi.deleteSteamApiKey).toHaveBeenCalledTimes(1));
  });

  it("confirma el impacto y destino antes de eliminar estados y columnas", async () => {
    const user = userEvent.setup();
    renderSettings({
      ...bootstrap,
      statuses: [
        {
          id: "unclassified",
          name: "Sin clasificar",
          color: "#71828E",
          position: 0,
          builtIn: true,
          gameCount: 0,
        },
        {
          id: "paused",
          name: "En pausa",
          color: "#D6A64B",
          position: 1,
          builtIn: false,
          gameCount: 2,
        },
      ],
      planner: [
        {
          id: "now",
          name: "Ahora",
          color: "#5CAAC1",
          position: 0,
          items: [{ appId: 10, title: "Vindexa QA", progress: 40, position: 0 }],
        },
        { id: "later", name: "Después", color: "#A4D007", position: 1, items: [] },
      ],
    });
    await user.click(screen.getByRole("button", { name: "Organización" }));

    await user.click(screen.getByRole("button", { name: "Eliminar En pausa" }));
    let dialog = screen.getByRole("alertdialog");
    expect(within(dialog).getByText(/2 juegos se reasignarán a “Sin clasificar”/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancelar" }));
    expect(mockedApi.deleteStatus).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Eliminar En pausa" }));
    dialog = screen.getByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Eliminar estado" }));
    await waitFor(() =>
      expect(mockedApi.deleteStatus).toHaveBeenCalledWith("paused", "unclassified"),
    );

    await user.click(screen.getByRole("button", { name: "Eliminar Ahora" }));
    dialog = screen.getByRole("alertdialog");
    expect(within(dialog).getByText(/1 juego se moverá a “Después”/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancelar" }));
    expect(mockedApi.deletePlannerColumn).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Eliminar Ahora" }));
    dialog = screen.getByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Eliminar columna" }));
    await waitFor(() => expect(mockedApi.deletePlannerColumn).toHaveBeenCalledWith("now", "later"));
  });

  it("confirma la restauración antes de abrir el selector del sistema", async () => {
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Datos y copias" }));

    await user.click(screen.getByRole("button", { name: "Restaurar copia" }));
    let dialog = screen.getByRole("alertdialog");
    expect(within(dialog).getByText(/snapshot de seguridad de la base actual/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancelar" }));
    expect(mockedApi.importBackup).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Restaurar copia" }));
    dialog = screen.getByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Elegir copia y restaurar" }));
    await waitFor(() => expect(mockedApi.importBackup).toHaveBeenCalledTimes(1));
  });

  it("expone comportamiento real y permite desactivar la confirmación de desinstalación", async () => {
    const user = userEvent.setup();
    renderSettings();

    const steamSection = screen.getByRole("button", { name: "Steam" });
    const appearanceSection = screen.getByRole("button", { name: "Apariencia" });
    expect(steamSection).toHaveAttribute("aria-current", "page");

    await user.click(appearanceSection);
    expect(appearanceSection).toHaveAttribute("aria-current", "page");
    expect(steamSection).not.toHaveAttribute("aria-current");

    expect(screen.getByRole("combobox", { name: "Densidad de la interfaz" })).toBeVisible();
    expect(screen.getByRole("combobox", { name: "Intervalo de sincronización" })).toBeVisible();
    const confirmation = screen.getByRole("switch", { name: "Confirmar desinstalación" });
    expect(confirmation).toBeChecked();
    await user.click(confirmation);
    await waitFor(() =>
      expect(mockedApi.savePreferences).toHaveBeenCalledWith({
        ...bootstrap.preferences,
        confirmUninstall: false,
      }),
    );
  });

  it("detecta colisiones al grabar atajos y persiste una combinación válida", async () => {
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Atajos" }));

    await user.click(screen.getByRole("button", { name: /Cambiar Biblioteca/ }));
    fireEvent.keyDown(window, { key: "2", metaKey: true });
    expect(await screen.findByRole("alert")).toHaveTextContent(/ya está asignado a Planificador/);
    expect(mockedApi.savePreferences).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /Cambiar Biblioteca/ }));
    fireEvent.keyDown(window, { key: "9", metaKey: true });
    await waitFor(() =>
      expect(mockedApi.savePreferences).toHaveBeenCalledWith({
        ...bootstrap.preferences,
        shortcuts: { ...bootstrap.preferences.shortcuts, library: "Mod+9" },
      }),
    );
  });

  it("comprueba actualizaciones manualmente sin prometer descarga ni firma", async () => {
    const user = userEvent.setup();
    mockedApi.checkForUpdates.mockResolvedValue({
      status: "notConfigured",
      currentVersion: "0.1.0",
      message: "No hay endpoint de versiones ni clave pública configurados.",
    });
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Acerca de" }));
    await user.click(screen.getByRole("button", { name: "Buscar actualizaciones" }));

    expect(await screen.findByText(/No hay endpoint de versiones/)).toBeVisible();
    expect(mockedApi.checkForUpdates).toHaveBeenCalledTimes(1);
  });
});
