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
    backupStatus: vi.fn(),
    exportBackup: vi.fn(),
    importBackup: vi.fn(),
    clearArtCache: vi.fn(),
    refreshSteamArt: vi.fn(),
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
    drmFreeGames: 0,
    drmPendingGames: 0,
    archivedGames: 0,
    familyCatalogGames: 0,
    externalStoreGames: {},
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
    artCacheMib: 0,
    shortcuts: {
      library: "Mod+1",
      planner: "Mod+2",
      collections: "Mod+3",
      tracking: "Mod+4",
      wishlist: "Mod+5",
      search: "Mod+K",
      sync: "Mod+Shift+S",
      closePanel: "Escape",
    },
  },
  appVersion: "0.0.0-pruebas",
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
    mockedApi.importLocalSteam.mockResolvedValue({
      steamPath: "/Applications/Steam.app",
      librariesScanned: 2,
      importedGames: 12,
      updatedGames: 0,
    });
    mockedApi.deleteStatus.mockResolvedValue(undefined);
    mockedApi.deletePlannerColumn.mockResolvedValue(undefined);
    mockedApi.importBackup.mockResolvedValue(false);
    mockedApi.backupStatus.mockResolvedValue({
      directory: "/Users/prueba/Library/Application Support/io.vindexa.desktop/copias",
      kept: 3,
      totalBytes: 54_000_000,
      lastAt: "2026-08-20T03:00:00Z",
      lastError: null,
    });
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
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["family-catalog"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library-filter-options"] });
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
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["family-catalog"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library-filter-options"] });
  });

  it("refresca todos los datos derivados incluso si la sincronización falla", async () => {
    const user = userEvent.setup();
    mockedApi.syncSteamLibrary.mockRejectedValueOnce(new Error("Steam no respondió."));
    const { queryClient } = renderSettings({
      ...bootstrap,
      steam: {
        ...bootstrap.steam,
        apiKeyConfigured: true,
        account: { steamId: "76561198000000000" },
      },
    });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    await user.click(screen.getByRole("button", { name: /Sincronizar ahora/ }));

    expect(await screen.findByText("Steam no respondió.")).toBeVisible();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["bootstrap"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["family-catalog"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library-filter-options"] });
  });

  it("refresca biblioteca, catálogo familiar y filtros tras importar Steam local", async () => {
    const user = userEvent.setup();
    const { queryClient } = renderSettings();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    await user.click(screen.getByRole("button", { name: /Explorar bibliotecas locales/ }));

    expect(await screen.findByText(/Se han leído 12 manifiestos/)).toBeVisible();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["bootstrap"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["family-catalog"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library-filter-options"] });
  });

  it("bloquea cambios de credenciales mientras Steam está sincronizando", async () => {
    const user = userEvent.setup();
    mockedApi.syncSteamLibrary.mockReturnValue(new Promise(() => undefined));
    renderSettings({
      ...bootstrap,
      steam: {
        ...bootstrap.steam,
        apiKeyConfigured: true,
        account: { steamId: "76561198000000000" },
      },
    });

    await user.click(screen.getByRole("button", { name: /Sincronizar ahora/ }));

    expect(screen.getByRole("button", { name: "Eliminar clave" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Desvincular" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Guardar y sincronizar/ })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Eliminar clave" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(mockedApi.deleteSteamApiKey).not.toHaveBeenCalled();
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
          items: [{ appId: 10, title: "Vindexa QA", progress: 40, position: 0, queuePosition: 0 }],
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

  it("Esc cancela la grabación de un atajo en vez de asignarse", async () => {
    // Mientras se graba, un manejador en captura se queda con todas las teclas.
    // Sin una salida, cambiar de opinión obliga a asignar algo que no se quiere
    // o a cerrar el diálogo; y Esc, que es el gesto de cancelar en cualquier
    // sistema, acababa asignándose o dando un error de colisión.
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Atajos" }));

    const boton = screen.getByRole("button", { name: /Cambiar Biblioteca/ });
    await user.click(boton);
    expect(boton).toHaveTextContent("Pulsa una combinación…");

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => expect(boton).not.toHaveTextContent("Pulsa una combinación…"));
    expect(within(boton).getByText(/1$/).tagName).toBe("KBD");
    expect(mockedApi.savePreferences).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("volver a pulsar el botón también cancela", async () => {
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Atajos" }));

    const boton = screen.getByRole("button", { name: /Cambiar Biblioteca/ });
    await user.click(boton);
    expect(boton).toHaveTextContent("Pulsa una combinación…");
    await user.click(boton);

    await waitFor(() => expect(boton).not.toHaveTextContent("Pulsa una combinación…"));
    expect(within(boton).getByText(/1$/).tagName).toBe("KBD");
    expect(mockedApi.savePreferences).not.toHaveBeenCalled();
  });

  it("Esc con modificador sí se puede asignar", async () => {
    // Cancelar es sólo el Esc a secas: si se descartara cualquier Esc, se
    // perdería una combinación legítima sin decirlo.
    const user = userEvent.setup();
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Atajos" }));

    await user.click(screen.getByRole("button", { name: /Cambiar Biblioteca/ }));
    fireEvent.keyDown(window, { key: "Escape", shiftKey: true });

    await waitFor(() =>
      expect(mockedApi.savePreferences).toHaveBeenCalledWith({
        ...bootstrap.preferences,
        shortcuts: { ...bootstrap.preferences.shortcuts, library: "Shift+Escape" },
      }),
    );
  });

  it("comprueba actualizaciones manualmente sin prometer descarga ni firma", async () => {
    const user = userEvent.setup();
    mockedApi.checkForUpdates.mockResolvedValue({
      status: "upToDate",
      currentVersion: "0.1.0",
      availableVersion: "0.1.0",
      message: "Estás en la versión 0.1.0, que es la última publicada.",
      releasePage: "https://github.com/SwonDev/Vindexa/releases/latest",
    });
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Acerca de" }));
    await user.click(screen.getByRole("button", { name: "Buscar actualizaciones" }));

    expect(await screen.findByText(/es la última publicada/)).toBeVisible();
    expect(mockedApi.checkForUpdates).toHaveBeenCalledTimes(1);
    // Al día no ofrece descargar nada: el enlace sólo aparece si hay versión
    // nueva de verdad.
    expect(screen.queryByRole("button", { name: /Ver la versión publicada/ })).toBeNull();
  });

  it("con versión nueva ofrece abrirla, y nunca la descarga sola", async () => {
    const user = userEvent.setup();
    mockedApi.checkForUpdates.mockResolvedValue({
      status: "available",
      currentVersion: "0.1.0",
      availableVersion: "0.1.1",
      message: "Hay una versión nueva: 0.1.1.",
      releasePage: "https://github.com/SwonDev/Vindexa/releases/latest",
    });
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Acerca de" }));
    await user.click(screen.getByRole("button", { name: "Buscar actualizaciones" }));

    expect(await screen.findByText(/Hay una versión nueva: 0\.1\.1/)).toBeVisible();
    expect(screen.getByRole("button", { name: /Ver la versión publicada/ })).toBeVisible();
  });

  it("no poder comprobarlo no se presenta como estar al día", async () => {
    const user = userEvent.setup();
    mockedApi.checkForUpdates.mockResolvedValue({
      status: "unreachable",
      currentVersion: "0.1.0",
      message: "No se ha podido consultar la página de versiones.",
      releasePage: "https://github.com/SwonDev/Vindexa/releases/latest",
    });
    renderSettings();
    await user.click(screen.getByRole("button", { name: "Acerca de" }));
    await user.click(screen.getByRole("button", { name: "Buscar actualizaciones" }));

    const aviso = await screen.findByText(/No se ha podido consultar/);
    expect(aviso).toBeVisible();
    expect(screen.queryByRole("button", { name: /Ver la versión publicada/ })).toBeNull();
  });
});

/**
 * La página de privacidad.
 *
 * Enumerar sólo lo que se queda aquí es media verdad: Vindexa pregunta a las
 * tiendas y para algunas cosas les dice de qué juegos habla. Lo que sale se
 * dice con la misma claridad que lo que se queda, o la página engaña por
 * omisión.
 */
describe("privacidad", () => {
  it("dice también qué sale del equipo, no sólo qué se queda", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(screen.getByRole("button", { name: "Privacidad" }));

    expect(await screen.findByText("Privacidad por diseño")).toBeVisible();
    expect(screen.getByText("Qué sale de este equipo")).toBeVisible();
    expect(screen.getByText(/Los AppID de tus juegos y deseados/)).toBeVisible();
    expect(screen.getByText(/sólo a Steam y sólo si vinculas la/)).toBeVisible();
    expect(screen.getByText(/Nada tuyo/)).toBeVisible();
    expect(screen.getByText(/ni telemetría, ni cuenta de Vindexa/)).toBeVisible();
  });

  /**
   * La frase entera tiene que ser **un** elemento.
   *
   * El contenedor es `flex`. Con los trozos sueltos como hijos directos, cada
   * uno se convertía en una columna: el «Los AppID de tus juegos y deseados» se
   * apilaba en vertical en una columna estrecha y el resto de la frase empezaba
   * a su lado. Se leía a saltos, y ninguna prueba de texto lo veía.
   */
  it("cada punto es un icono y una frase, no cuatro columnas", async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(screen.getByRole("button", { name: "Privacidad" }));
    await screen.findByText("Qué sale de este equipo");

    // El diálogo vive en un portal, así que se busca en el documento entero.
    const puntos = document.querySelectorAll('.privacy-list[data-tone="outbound"] li');
    expect(puntos).toHaveLength(4);
    for (const punto of puntos) {
      const hijos = Array.from(punto.children);
      expect(hijos).toHaveLength(2);
      expect(hijos[0]?.tagName.toLowerCase()).toBe("svg");
      expect(hijos[1]?.tagName.toLowerCase()).toBe("span");
      // Y no queda texto suelto fuera de la frase.
      const sueltos = Array.from(punto.childNodes).filter(
        (nodo) => nodo.nodeType === Node.TEXT_NODE && nodo.textContent?.trim(),
      );
      expect(sueltos).toHaveLength(0);
    }
  });
});

/**
 * Las copias automáticas.
 *
 * Lo que hace valiosa esta base es irrepetible y no está en ningún servidor.
 * Había exportación manual, que es lo mismo que no tener copias: nadie pulsa un
 * botón todos los días. Y una copia que dejó de hacerse **en silencio** sólo se
 * descubre el día que se necesita, así que el fallo tiene que verse.
 */
/**
 * El arte se puede volver a contrastar sin esperar doce horas.
 *
 * La orden existía en Rust desde que el índice oficial arregló 445 carátulas
 * rotas —«quien vea una carátula rota puede forzar la corrección sin esperar»,
 * decía su documentación— y no la llamaba nadie: no estaba en `api` ni había
 * botón. La pasada automática corre cada doce horas, así que ver algo mal
 * significaba esperar.
 */
describe("contrastar el arte con la tienda", () => {
  it("dice cuántos se corrigieron y cuántos no tienen arte publicada", async () => {
    const user = userEvent.setup();
    mockedApi.refreshSteamArt.mockResolvedValue({
      examined: 3877,
      resolved: 3501,
      updated: 1786,
      withoutArt: 376,
      failedBatches: 0,
    });
    renderSettings();

    await user.click(screen.getByRole("button", { name: "Datos y copias" }));
    await user.click(await screen.findByRole("button", { name: /Contrastar arte con la tienda/ }));

    expect(await screen.findByText(/3.501 de 3.877|3501 de 3877/)).toBeVisible();
    expect(screen.getByText(/376 sin arte publicada/)).toBeVisible();
  });

  it("un lote sin respuesta se cuenta en vez de pasar por completo", async () => {
    const user = userEvent.setup();
    mockedApi.refreshSteamArt.mockResolvedValue({
      examined: 400,
      resolved: 200,
      updated: 12,
      withoutArt: 0,
      failedBatches: 1,
    });
    renderSettings();

    await user.click(screen.getByRole("button", { name: "Datos y copias" }));
    await user.click(await screen.findByRole("button", { name: /Contrastar arte con la tienda/ }));

    expect(await screen.findByText(/1 lote sin respuesta/)).toBeVisible();
  });
});

describe("copias automáticas", () => {
  it("dice cuándo fue la última, cuántas hay y dónde están", async () => {
    const user = userEvent.setup();
    mockedApi.backupStatus.mockResolvedValue({
      directory: "/Users/prueba/Library/Application Support/io.vindexa.desktop/copias",
      kept: 3,
      totalBytes: 54_000_000,
      lastAt: "2026-08-20T03:00:00Z",
      lastError: null,
    });
    renderSettings();

    await user.click(screen.getByRole("button", { name: "Datos y copias" }));

    expect(await screen.findByText("Copias automáticas")).toBeVisible();
    expect(await screen.findByText("3 de 3")).toBeVisible();
    expect(screen.getByLabelText("Dónde se guardan")).toHaveValue(
      "/Users/prueba/Library/Application Support/io.vindexa.desktop/copias",
    );
  });

  it("un fallo de la última copia no se calla", async () => {
    const user = userEvent.setup();
    mockedApi.backupStatus.mockResolvedValue({
      directory: "/tmp/copias",
      kept: 1,
      totalBytes: 1_024,
      lastAt: "2026-08-18T03:00:00Z",
      lastError: "No queda espacio en el disco.",
    });
    renderSettings();

    await user.click(screen.getByRole("button", { name: "Datos y copias" }));
    expect(await screen.findByText(/No queda espacio en el disco/)).toBeVisible();
  });
});
