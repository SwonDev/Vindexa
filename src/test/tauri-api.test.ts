import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppPreferences, GameListRequest, UpdateGameInput } from "@/lib/types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("contrato frontend de comandos Tauri", () => {
  beforeEach(() => {
    invokeMock.mockResolvedValue(undefined);
  });

  it("mantiene los filtros dentro del argumento request", async () => {
    const request: GameListRequest = {
      query: "aventura",
      statusId: "playing",
      installed: true,
      neverPlayed: false,
      minPlaytimeMinutes: 120,
      genre: "Acción",
      releaseFrom: "2024-01-01",
      maxSessionMinutes: 90,
      sort: "lastPlayed",
      limit: 60,
      offset: 0,
    };

    await api.listGames(request);

    expect(invokeMock).toHaveBeenCalledWith("list_games", { request });
  });

  it("carga las opciones de filtro sin descargar juegos al frontend", async () => {
    await api.libraryFilterOptions();

    expect(invokeMock).toHaveBeenCalledWith("get_library_filter_options");
  });

  it("prioriza solo la página visible y deja el resto en la cola incremental", async () => {
    await api.startMetadataEnrichment([620, 730], true);
    await api.metadataEnrichmentStatus();

    expect(invokeMock).toHaveBeenNthCalledWith(1, "start_metadata_enrichment", {
      visibleAppIds: [620, 730],
      includeBacklog: true,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "metadata_enrichment_status");
  });

  it("pagina el historial personal sin ocultar sesiones antiguas", async () => {
    await api.listGameSessions(620, 50, 100);

    expect(invokeMock).toHaveBeenCalledWith("list_game_sessions", {
      appId: 620,
      limit: 50,
      offset: 100,
    });
  });

  it("envía la edición completa como un único input tipado", async () => {
    const input: UpdateGameInput = {
      appId: 620,
      statusId: "playing",
      progress: 65,
      priority: 4,
      pinned: true,
      tracking: true,
      rating: 9,
      estimatedMinutes: 180,
      targetDate: "2026-09-01",
      nextAction: "Completar el acto actual",
      checkpoint: "Campamento",
      notes: "No perder durante la sincronización",
    };

    await api.updateGame(input);

    expect(invokeMock).toHaveBeenCalledWith("update_game", { input });
  });

  it("envía el cambio de estado masivo en una sola invocación", async () => {
    const input = { appIds: [10, 20, 30], statusId: "playing" };

    await api.bulkUpdateStatus(input);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("bulk_update_status", input);
  });

  it("serializa el movimiento del planificador con nombres camelCase", async () => {
    await api.movePlannerItem(10, "now", 3);

    expect(invokeMock).toHaveBeenCalledWith("move_planner_item", {
      input: { appId: 10, columnId: "now", position: 3 },
    });
  });

  it("solo consulta la clave guardada mediante el comando explícito", async () => {
    await api.verifySavedSteamApiKey();

    expect(invokeMock).toHaveBeenCalledWith("verify_saved_steam_api_key");
  });

  it("separa operaciones sensibles de cuenta, clave y copia de seguridad", async () => {
    const preferences: AppPreferences = {
      density: "compact",
      periodicSyncMinutes: 30,
      confirmUninstall: true,
      librarySort: "lastPlayed",
      shortcuts: {
        library: "Mod+1",
        planner: "Mod+2",
        collections: "Mod+3",
        tracking: "Mod+4",
        search: "Mod+K",
        sync: "Mod+Shift+S",
        closePanel: "Escape",
      },
    };

    await api.saveSteamApiKey("clave-solo-para-el-mock");
    await api.unlinkSteam();
    await api.exportBackup();
    await api.importBackup();
    await api.savePreferences(preferences);

    expect(invokeMock).toHaveBeenNthCalledWith(1, "save_steam_api_key", {
      apiKey: "clave-solo-para-el-mock",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "unlink_steam");
    expect(invokeMock).toHaveBeenNthCalledWith(3, "export_backup");
    expect(invokeMock).toHaveBeenNthCalledWith(4, "import_backup");
    expect(invokeMock).toHaveBeenNthCalledWith(5, "save_preferences", { preferences });
  });

  it("solicita la desinstalación a Steam y comprueba actualizaciones sin instalarlas", async () => {
    await api.uninstallGame(620);
    await api.checkForUpdates();

    expect(invokeMock).toHaveBeenNthCalledWith(1, "uninstall_game", { appId: 620 });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "check_for_updates");
  });

  it("normaliza errores sin filtrar objetos internos a la interfaz", () => {
    expect(getErrorMessage("Biblioteca privada")).toBe("Biblioteca privada");
    expect(getErrorMessage(new Error("Tiempo de espera agotado"))).toBe("Tiempo de espera agotado");
    expect(getErrorMessage({ code: "steam_unavailable" })).toBe("steam_unavailable");
    expect(getErrorMessage({ internal: "detalle no presentable" })).toBe(
      "Vindexa no pudo completar la operación.",
    );
  });
});
