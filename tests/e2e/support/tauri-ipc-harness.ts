import type {
  AppPreferences,
  GameListRequest,
  LibraryDropInput,
  LibraryDropReceipt,
  UpdateGameInput,
} from "../../../src/lib/types";
import type { TestBackendState } from "./test-data";

export function installTauriIpcHarness(seed: TestBackendState) {
  // addInitScript también se evalúa sobre about:blank, donde Chromium bloquea localStorage.
  if (window.location.protocol !== "http:" && window.location.protocol !== "https:") return;
  const storageKey = "vindexa:e2e:backend:v1";
  const read = (): TestBackendState => {
    const stored = window.localStorage.getItem(storageKey);
    return stored ? JSON.parse(stored) : structuredClone(seed);
  };
  const write = (state: TestBackendState) => {
    window.localStorage.setItem(storageKey, JSON.stringify(state));
  };
  if (!window.localStorage.getItem(storageKey)) write(structuredClone(seed));

  const hydrateBootstrap = (state: TestBackendState) => {
    const next = structuredClone(state.bootstrap);
    next.stats = {
      totalGames: state.games.length,
      installedGames: state.games.filter((game) => game.installed).length,
      playingGames: state.games.filter((game) => game.statusId === "playing").length,
      backlogGames: state.games.filter((game) => game.progress < 100).length,
      trackedGames: state.games.filter((game) => game.tracking).length,
      totalPlaytimeMinutes: state.games.reduce((sum, game) => sum + game.playtimeMinutes, 0),
    };
    next.statuses = next.statuses.map((status) => ({
      ...status,
      gameCount: state.games.filter((game) => game.statusId === status.id).length,
    }));
    next.collections = next.collections.map((collection) => ({
      ...collection,
      gameCount:
        collection.kind === "manual"
          ? (state.collections[collection.id]?.length ?? 0)
          : collection.gameCount,
    }));
    return next;
  };

  let callbackId = 0;
  const callbacks = new Map<number, (...args: unknown[]) => void>();
  const transformCallback = (callback: (...args: unknown[]) => void, once = false) => {
    callbackId += 1;
    const id = callbackId;
    callbacks.set(
      id,
      once
        ? (...args: unknown[]) => {
            callbacks.delete(id);
            callback(...args);
          }
        : callback,
    );
    return id;
  };

  const invoke = async (command: string, args: Record<string, unknown> = {}) => {
    const state = read();
    state.commandLog.push(command);
    if (state.commandLog.length > 200) state.commandLog.shift();
    write(state);

    if (command === "get_database_recovery_status") {
      if (state.scenario !== "database-recovery") {
        return { required: false, backups: [], recoveryActionsAvailable: false };
      }
      return {
        required: true,
        issue: { code: "database_integrity", message: "SQLite detectó daños en la copia aislada." },
        quarantine: {
          id: "e2e-quarantine",
          detectedAt: "2026-08-14T12:00:00Z",
          fileName: "vindexa.sqlite3",
          sizeBytes: 4_096,
          sidecarCount: 1,
          integrity: "database disk image is malformed",
          schemaVersion: 13,
        },
        backups: [
          {
            id: "e2e-backup",
            label: "Copia automática verificada",
            sizeBytes: 8_192,
            modifiedAt: "2026-08-14T11:00:00Z",
            source: "safety",
            valid: true,
            validationMessage: "Identidad, esquema, relaciones e integridad verificados.",
          },
        ],
        recoveryActionsAvailable: true,
      };
    }
    if (command === "restore_database_recovery_backup") {
      if (args.confirmation !== "RESTAURAR") throw new Error("Confirmación incorrecta.");
      state.scenario = "library";
      write(state);
      return { required: false, backups: [], recoveryActionsAvailable: false };
    }
    if (command === "create_clean_database_after_recovery") {
      if (args.confirmation !== "CREAR NUEVA") throw new Error("Confirmación incorrecta.");
      state.scenario = "empty";
      state.games = [];
      state.bootstrap.collections = [];
      write(state);
      return { required: false, backups: [], recoveryActionsAvailable: false };
    }
    if (command === "refresh_database_recovery_backups") {
      return invoke("get_database_recovery_status", {});
    }
    if (command === "select_database_recovery_backup") {
      return invoke("get_database_recovery_status", {});
    }
    if (command === "export_quarantined_database") return true;
    if (command === "bootstrap") {
      if (state.scenario === "startup-error") {
        throw new Error("SQLite de pruebas no está disponible.");
      }
      if (state.bootstrapFailuresRemaining > 0) {
        state.bootstrapFailuresRemaining -= 1;
        write(state);
        throw new Error("El arranque aislado falló una vez.");
      }
      return hydrateBootstrap(state);
    }
    if (command === "get_library_filter_options") {
      return {
        genres: ["Aventura", "Exploración"],
        categories: ["Un jugador", "Compatibilidad con mando"],
        tags: [],
        totalGames: state.games.length,
        metadataGames: state.games.length,
        achievementGames: state.games.length,
        steamDeckGames: state.games.length,
      };
    }
    if (command === "metadata_enrichment_status" || command === "start_metadata_enrichment") {
      return {
        running: false,
        totalGames: state.games.length,
        freshMetadata: state.games.length,
        queued: 0,
        processing: 0,
        retrying: 0,
        succeeded: 0,
        unavailable: 0,
        failed: 0,
        steamDeckAvailability: "disabled",
        steamDeckExplanation: "Fixture E2E sin red.",
      };
    }
    if (command === "list_games") {
      if (state.scenario === "library-error") {
        throw new Error("La consulta aislada de biblioteca falló.");
      }
      const request = (args.request ?? {}) as GameListRequest;
      let items = [...state.games];
      if (request.query) {
        const query = String(request.query).toLocaleLowerCase("es-ES");
        items = items.filter((game) => game.title.toLocaleLowerCase("es-ES").includes(query));
      }
      if (request.statusId) items = items.filter((game) => game.statusId === request.statusId);
      if (request.collectionId) {
        const ids = state.collections[String(request.collectionId)] ?? [];
        items = items.filter((game) => ids.includes(game.appId));
      }
      if (typeof request.installed === "boolean") {
        items = items.filter((game) => game.installed === request.installed);
      }
      if (request.tracking) items = items.filter((game) => game.tracking);
      if (request.sort === "alphabetical") {
        items.sort((left, right) => left.title.localeCompare(right.title, "es"));
      } else if (request.sort === "manual") {
        items.sort((left, right) => left.manualPosition - right.manualPosition);
      }
      const offset = Number(request.offset ?? 0);
      const limit = Number(request.limit ?? 240);
      return { items: items.slice(offset, offset + limit), total: items.length, limit, offset };
    }
    if (command === "get_game_detail") {
      const detail = state.games.find((game) => game.appId === Number(args.appId));
      if (!detail) throw new Error("El juego solicitado no existe en el fixture.");
      return structuredClone(detail);
    }
    if (command === "refresh_game_metadata" || command === "refresh_game_achievements") {
      const detail = state.games.find((game) => game.appId === Number(args.appId));
      if (!detail) throw new Error("El juego solicitado no existe en el fixture.");
      return structuredClone(detail);
    }
    if (command === "update_game") {
      const input = args.input as UpdateGameInput;
      const index = state.games.findIndex((game) => game.appId === Number(input.appId));
      if (index < 0) throw new Error("No se pudo guardar el juego del fixture.");
      const status = state.bootstrap.statuses.find((item) => item.id === input.statusId);
      state.games[index] = {
        ...state.games[index],
        ...input,
        statusName: status?.name ?? state.games[index].statusName,
        statusColor: status?.color ?? state.games[index].statusColor,
      };
      write(state);
      return structuredClone(state.games[index]);
    }
    if (command === "apply_library_drop") {
      const input = args.input as LibraryDropInput;
      const appIds = input.appIds.map(Number);
      if (input.target.kind === "status") {
        const previous = state.games
          .filter((game) => appIds.includes(game.appId))
          .map((game) => ({ appId: game.appId, statusId: game.statusId }));
        const status = state.bootstrap.statuses.find((item) => item.id === input.target.id);
        state.games = state.games.map((game) =>
          appIds.includes(game.appId)
            ? {
                ...game,
                statusId: input.target.id,
                statusName: status?.name ?? game.statusName,
                statusColor: status?.color ?? game.statusColor,
              }
            : game,
        );
        write(state);
        return {
          moved: appIds.length,
          receipt: {
            kind: "status",
            operationId: "e2e-status-drop",
            targetId: input.target.id,
            appIds,
            previous,
            activityIds: [],
          },
        };
      }
      if (input.target.kind === "collection") {
        const previousOrder = [...(state.collections[input.target.id] ?? [])];
        const withoutMoved = previousOrder.filter((id) => !appIds.includes(id));
        const beforeIndex = input.target.beforeAppId
          ? withoutMoved.indexOf(input.target.beforeAppId)
          : -1;
        const appliedOrder =
          beforeIndex >= 0
            ? [...withoutMoved.slice(0, beforeIndex), ...appIds, ...withoutMoved.slice(beforeIndex)]
            : [...withoutMoved, ...appIds];
        state.collections[input.target.id] = appliedOrder;
        write(state);
        return {
          moved: appIds.length,
          receipt: {
            kind: "collection",
            operationId: "e2e-collection-drop",
            targetId: input.target.id,
            appIds,
            beforeAppId: input.target.beforeAppId,
            previousOrder,
            appliedOrder,
          },
        };
      }
      const previousOrder = state.games
        .slice()
        .sort((left, right) => left.manualPosition - right.manualPosition)
        .map((game) => game.appId);
      const withoutMoved = previousOrder.filter((id) => !appIds.includes(id));
      const beforeIndex = withoutMoved.indexOf(input.target.beforeAppId);
      const appliedOrder = [
        ...withoutMoved.slice(0, Math.max(0, beforeIndex)),
        ...appIds,
        ...withoutMoved.slice(Math.max(0, beforeIndex)),
      ];
      state.games = state.games.map((game) => ({
        ...game,
        manualPosition: appliedOrder.indexOf(game.appId),
      }));
      write(state);
      return {
        moved: appIds.length,
        receipt: {
          kind: "manual",
          operationId: "e2e-manual-drop",
          appIds,
          beforeAppId: input.target.beforeAppId,
          previousOrder,
          appliedOrder,
        },
      };
    }
    if (command === "undo_library_drop") {
      const receipt = args.receipt as LibraryDropReceipt;
      if (receipt.kind === "status") {
        const previous = new Map(
          receipt.previous.map((item) => [Number(item.appId), item.statusId]),
        );
        state.games = state.games.map((game) => {
          const statusId = previous.get(game.appId);
          if (!statusId) return game;
          const status = state.bootstrap.statuses.find((item) => item.id === statusId);
          return {
            ...game,
            statusId,
            statusName: status?.name ?? game.statusName,
            statusColor: status?.color ?? game.statusColor,
          };
        });
      } else if (receipt.kind === "collection") {
        state.collections[receipt.targetId] = receipt.previousOrder;
      } else {
        state.games = state.games.map((game) => ({
          ...game,
          manualPosition: receipt.previousOrder.indexOf(game.appId),
        }));
      }
      write(state);
      return receipt.appIds.length;
    }
    if (command === "set_game_collections") {
      const appId = Number(args.appId);
      const collectionIds = args.collectionIds as string[];
      for (const collection of state.bootstrap.collections.filter(
        (item) => item.kind === "manual",
      )) {
        const current = state.collections[collection.id] ?? [];
        state.collections[collection.id] = collectionIds.includes(collection.id)
          ? Array.from(new Set([...current, appId]))
          : current.filter((id) => id !== appId);
      }
      const detail = state.games.find((game) => game.appId === appId);
      if (!detail) throw new Error("El juego solicitado no existe en el fixture.");
      detail.collectionIds = [...collectionIds];
      write(state);
      return structuredClone(detail);
    }
    if (command === "reorder_collections") {
      const byId = new Map(state.bootstrap.collections.map((item) => [item.id, item]));
      state.bootstrap.collections = (args.ids as string[]).flatMap((id, position) => {
        const item = byId.get(id);
        return item ? [{ ...item, position }] : [];
      });
      write(state);
      return;
    }
    if (command === "save_preferences") {
      state.bootstrap.preferences = structuredClone(args.preferences as AppPreferences);
      write(state);
      return;
    }
    if (command === "get_planner_overview") return structuredClone(state.planner);
    if (command === "list_family_catalog") {
      return { items: [], total: 0, limit: 240, offset: 0 };
    }
    if (command === "list_tags") return [];
    if (command === "list_game_sessions") {
      return {
        items: [],
        total: 0,
        limit: Number(args.limit ?? 50),
        offset: Number(args.offset ?? 0),
      };
    }
    if (command === "get_discovery_snapshot") {
      return {
        reminders: [],
        forgotten: [],
        almostFinished: [],
        upcoming: [],
        events: [],
        officialPublications: [],
        relatedReleases: [],
        dismissedRecommendations: [],
        capabilities: {
          metadataObservations: 0,
          earlyAccessHistoryAvailable: false,
          trackedNewsGames: 0,
          officialPublicationsAvailable: false,
          relatedReleasesAvailable: false,
        },
      };
    }
    if (command === "cache_game_art") throw new Error("E2E_ARTWORK_OFFLINE");
    if (command === "import_local_steam") {
      return { steamPath: "/e2e/steam", librariesScanned: 0, importedGames: 0, updatedGames: 0 };
    }
    if (
      [
        "launch_game",
        "install_game",
        "uninstall_game",
        "open_store",
        "reveal_installation",
        "move_planner_item",
        "move_planner_queue_item",
        "save_planner_item",
        "save_planner_capacity",
      ].includes(command)
    ) {
      return;
    }
    throw new Error(`Comando Tauri no simulado en E2E: ${command}`);
  };

  const tauriWindow = window as typeof window & {
    __TAURI_INTERNALS__?: Record<string, unknown>;
    __VINDEXA_E2E__?: { snapshot: () => TestBackendState };
  };
  tauriWindow.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback,
    unregisterCallback: (id: number) => callbacks.delete(id),
    runCallback: (id: number, data: unknown) => callbacks.get(id)?.(data),
    callbacks,
    convertFileSrc: (path: string) => path,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main", windowLabel: "main" },
    },
  };
  tauriWindow.__VINDEXA_E2E__ = { snapshot: read };
}
