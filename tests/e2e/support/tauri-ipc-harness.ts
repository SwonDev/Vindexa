import type {
  AppPreferences,
  DlcFilter,
  DlcSummary,
  GameDlc,
  GameListRequest,
  LibraryDropInput,
  LibraryDropReceipt,
  NotificationRule,
  PriorityExplanation,
  SaveNotificationRuleInput,
  TasteReport,
  UpcomingRelease,
  UpdateGameInput,
} from "../../../src/lib/types";
import type { TestBackendState } from "./test-data";

export function installTauriIpcHarness(seed: TestBackendState) {
  // addInitScript también se evalúa sobre about:blank, donde Chromium bloquea localStorage.
  if (window.location.protocol !== "http:" && window.location.protocol !== "https:") return;
  const storageKey = "vindexa:e2e:backend:v1";
  // Contenido adicional y prioridad viven en memoria: son estado de una sesión
  // de navegador, no del fixture persistido, y así cada prueba parte limpia.
  const dlcCatalog = new Map<number, GameDlc[]>();
  const priorityLocks = new Map<number, boolean>();
  // Avisos programados y próximos lanzamientos: también estado de sesión.
  const notificationRules = new Map<string, NotificationRule>();
  const upcomingReleases = new Map<number, UpcomingRelease>();
  let notificationsSeeded = false;
  /**
   * `scheduledFor` es el ancla inmutable y `nextOccurrence` se recalcula en cada
   * lectura, igual que hace `enrich_rule` en Rust. El paso mensual recorta al
   * último día del mes destino sin perder el día del ancla: 31/01 → 28/02 → 31/03.
   */
  const addMonthsClamped = (anchor: Date, months: number): Date => {
    const day = anchor.getUTCDate();
    const shifted = new Date(anchor.getTime());
    shifted.setUTCDate(1);
    shifted.setUTCMonth(shifted.getUTCMonth() + months);
    const lastDay = new Date(
      Date.UTC(shifted.getUTCFullYear(), shifted.getUTCMonth() + 1, 0),
    ).getUTCDate();
    shifted.setUTCDate(Math.min(day, lastDay));
    return shifted;
  };
  const nextOccurrenceOf = (rule: NotificationRule, now: Date): string | undefined => {
    if (!rule.scheduledFor) return undefined;
    const anchor = new Date(rule.scheduledFor);
    if (Number.isNaN(anchor.getTime())) return undefined;
    const horizon = now.getTime() + rule.leadMinutes * 60_000;
    if (anchor.getTime() > horizon) return anchor.toISOString();
    if (rule.repeatRule === "none") return undefined;
    const stepMs = rule.repeatRule === "daily" ? 86_400_000 : 604_800_000;
    for (let step = 1; step <= 1_200; step += 1) {
      const candidate =
        rule.repeatRule === "monthly"
          ? addMonthsClamped(anchor, step)
          : new Date(anchor.getTime() + step * stepMs);
      if (candidate.getTime() > horizon) return candidate.toISOString();
    }
    return undefined;
  };
  const decorateRule = (rule: NotificationRule): NotificationRule => {
    const { nextOccurrence: _previous, ...rest } = rule;
    const next = nextOccurrenceOf(rule, new Date());
    return next ? { ...rest, nextOccurrence: next } : rest;
  };
  const seedNotifications = (state: TestBackendState) => {
    if (notificationsSeeded) return;
    notificationsSeeded = true;
    if (state.scenario !== "showcase") return;
    const anchored = state.games[0];
    for (const rule of [
      {
        id: "rule-mensual",
        ...(anchored ? { appId: anchored.appId, gameTitle: anchored.title } : {}),
        kind: "manual" as const,
        title: "Repasar lo pendiente del mes",
        body: "",
        // Ancla del día 31: la cita de febrero cae el 28 y marzo vuelve al 31.
        scheduledFor: "2026-01-31T09:00:00Z",
        repeatRule: "monthly" as const,
        leadMinutes: 60,
        enabled: true,
        lastFiredAt: "2026-01-31T08:00:00Z",
        createdAt: "2026-01-01T09:00:00Z",
        updatedAt: "2026-01-31T08:00:00Z",
      },
      {
        id: "rule-lanzamiento",
        kind: "manual" as const,
        title: "Sale «Ruina Boreal» del acceso anticipado",
        body: "Comprobar si las partidas guardadas siguen siendo válidas.",
        scheduledFor: "2026-11-04T09:00:00Z",
        repeatRule: "none" as const,
        leadMinutes: 1_440,
        enabled: true,
        createdAt: "2026-08-01T09:00:00Z",
        updatedAt: "2026-08-01T09:00:00Z",
      },
      {
        id: "rule-pausada",
        kind: "manual" as const,
        title: "Revisar los deseados antes de rebajas",
        body: "",
        scheduledFor: "2026-03-01T18:00:00Z",
        repeatRule: "weekly" as const,
        leadMinutes: 0,
        enabled: false,
        createdAt: "2026-01-01T09:00:00Z",
        updatedAt: "2026-01-01T09:00:00Z",
      },
    ]) {
      notificationRules.set(rule.id, rule);
    }
    for (const item of [
      {
        appId: 990_001,
        title: "Ruina Boreal",
        capsuleUrl: null,
        headerUrl: null,
        releaseDate: "2026-11-04",
        releaseDateIsExact: true,
        genres: ["Metroidvania", "Acción"],
        categories: ["Un jugador"],
        developer: "Team Cherry",
        publisher: null,
        shortDescription: null,
        matchScore: 0.62,
        matchReason: "Coincide con tus 62 h en metroidvania y con Team Cherry.",
        source: "store" as const,
        dismissedAt: null,
        discoveredAt: "2026-08-01T10:00:00Z",
        updatedAt: "2026-08-01T10:00:00Z",
      },
      {
        appId: 990_002,
        title: "Cantera de Sal",
        capsuleUrl: null,
        headerUrl: null,
        // Etiqueta sin día concreto: la interfaz debe pintarla como aproximada.
        releaseDate: "Q4 2026",
        releaseDateIsExact: false,
        genres: ["Estrategia"],
        categories: ["Un jugador"],
        developer: "Estudio Lento",
        publisher: "Editorial Norte",
        shortDescription: null,
        matchScore: 0.31,
        matchReason: "Coincide con tus 18 h en estrategia por turnos.",
        source: "store" as const,
        dismissedAt: null,
        discoveredAt: "2026-08-01T10:00:00Z",
        updatedAt: "2026-08-01T10:00:00Z",
      },
      {
        appId: 990_003,
        title: "Proyecto sin anunciar",
        capsuleUrl: null,
        headerUrl: null,
        releaseDate: null,
        releaseDateIsExact: false,
        genres: [],
        categories: [],
        developer: null,
        publisher: null,
        shortDescription: null,
        matchScore: 0,
        matchReason: "Todavía no hay señales en tu biblioteca que lo relacionen con nada.",
        source: "store" as const,
        dismissedAt: null,
        discoveredAt: "2026-08-01T10:00:00Z",
        updatedAt: "2026-08-01T10:00:00Z",
      },
    ]) {
      upcomingReleases.set(item.appId, item);
    }
  };
  const seedDlc = (appId: number): GameDlc[] => {
    const existing = dlcCatalog.get(appId);
    if (existing) return existing;
    const base = (dlcAppId: number, title: string, extra: Partial<GameDlc>): GameDlc => ({
      appId,
      dlcAppId,
      title,
      isFree: false,
      owned: false,
      installed: false,
      hidden: false,
      metadataStatus: "success",
      position: dlcAppId % 10,
      updatedAt: "2026-08-18T09:00:00Z",
      ...extra,
    });
    const created =
      appId === 620
        ? [
            base(6201, "Banda sonora original", {
              releaseDate: "2011-05-24",
              priceCents: 649,
              currency: "EUR",
              owned: true,
              installed: true,
            }),
            base(6202, "Mapas de la comunidad", { priceCents: 1299, currency: "EUR" }),
            // Sin precio publicado: alimenta el «al menos» del importe pendiente.
            base(6203, "Sombrero de gala", { metadataStatus: "unavailable" }),
            // En otra moneda: tampoco entra en la suma.
            base(6204, "Pack retirado", { priceCents: 999, currency: "USD" }),
            base(6205, "Prueba cerrada", { isFree: true, hidden: true }),
          ]
        : [];
    dlcCatalog.set(appId, created);
    return created;
  };
  const dlcSummaryOf = (appId: number): DlcSummary => {
    const all = seedDlc(appId);
    const pending = all.filter((item) => !item.owned && !item.hidden && !item.isFree);
    const priced = pending.filter((item) => item.priceCents !== undefined && item.currency);
    const eur = priced.filter((item) => item.currency === "EUR");
    return {
      appId,
      total: all.length,
      owned: all.filter((item) => item.owned).length,
      installed: all.filter((item) => item.installed).length,
      hidden: all.filter((item) => item.hidden).length,
      free: all.filter((item) => item.isFree).length,
      pending: pending.length,
      ...(eur.length
        ? {
            pendingValueCents: eur.reduce((sum, item) => sum + (item.priceCents ?? 0), 0),
            pendingValueCurrency: "EUR",
          }
        : {}),
      pendingCounted: eur.length,
      pendingUnknownPrice: pending.length - priced.length,
      pendingOtherCurrency: priced.length - eur.length,
    };
  };
  const setDlcFlag = (
    appId: number,
    dlcAppId: number,
    patch: Partial<Pick<GameDlc, "owned" | "installed" | "hidden">>,
  ): GameDlc => {
    const item = seedDlc(appId).find((candidate) => candidate.dlcAppId === dlcAppId);
    if (!item) throw new Error("El contenido adicional solicitado no existe en el fixture.");
    Object.assign(item, patch);
    return structuredClone(item);
  };
  const familyCatalog = [
    {
      appId: 410,
      title: "Aurora Assembly",
      availability: "confirmed",
      discoveredAt: "2026-08-11T10:00:00Z",
      updatedAt: "2026-08-14T10:00:00Z",
    },
    {
      appId: 420,
      title: "Bastion of Moss",
      availability: "unknown",
      discoveredAt: "2026-08-14T09:00:00Z",
      updatedAt: "2026-08-14T09:00:00Z",
    },
    {
      appId: 430,
      title: "Clockwork Family",
      availability: "confirmed",
      discoveredAt: "2026-08-10T10:00:00Z",
      updatedAt: "2026-08-13T10:00:00Z",
    },
    {
      appId: 440,
      title: "Distant Gardens",
      availability: "unknown",
      discoveredAt: "2026-08-13T10:00:00Z",
      updatedAt: "2026-08-13T10:00:00Z",
    },
    {
      appId: 450,
      title: "Ember Family",
      availability: "confirmed",
      discoveredAt: "2026-08-09T10:00:00Z",
      updatedAt: "2026-08-12T10:00:00Z",
    },
    {
      appId: 460,
      title: "Frostline Workshop",
      availability: "unknown",
      discoveredAt: "2026-08-12T10:00:00Z",
      updatedAt: "2026-08-12T10:00:00Z",
    },
    {
      appId: 470,
      title: "Glass Harbor",
      availability: "confirmed",
      discoveredAt: "2026-08-08T10:00:00Z",
      updatedAt: "2026-08-11T10:00:00Z",
    },
    {
      appId: 480,
      title: "Hollow Meridian",
      availability: "unknown",
      discoveredAt: "2026-08-11T10:00:00Z",
      updatedAt: "2026-08-11T10:00:00Z",
    },
    {
      appId: 490,
      title: "Ivory Circuit",
      availability: "confirmed",
      discoveredAt: "2026-08-07T10:00:00Z",
      updatedAt: "2026-08-10T10:00:00Z",
    },
    {
      appId: 500,
      title: "Juniper Keep",
      availability: "unknown",
      discoveredAt: "2026-08-10T10:00:00Z",
      updatedAt: "2026-08-10T10:00:00Z",
    },
    {
      appId: 510,
      title: "Kestrel Foundry",
      availability: "confirmed",
      discoveredAt: "2026-08-06T10:00:00Z",
      updatedAt: "2026-08-09T10:00:00Z",
    },
    {
      appId: 520,
      title: "Lumen Orchard",
      availability: "unknown",
      discoveredAt: "2026-08-09T10:00:00Z",
      updatedAt: "2026-08-09T10:00:00Z",
    },
  ];
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

  /**
   * Deseados, listas curadas y vídeos del arnés.
   *
   * Viven en memoria durante la sesión de pruebas y no forman parte de
   * `TestBackendState`: así ninguna captura ni ninguna prueba existente cambia
   * de forma por añadirlos. `parseYoutubeId` reproduce a propósito la frontera
   * de seguridad de `parse_youtube_video_id` en `src-tauri/src/db/wishlist.rs`:
   * la URL de incrustación la construye el backend simulado, nunca la pantalla.
   */
  interface E2EWishlistEntry {
    appId: number;
    bucket: string;
    priority: number;
    position: number;
    note: string;
    targetPriceCents?: number;
    currency?: string;
  }
  interface E2ECuratedItem {
    appId: number;
    note: string;
    highlight: boolean;
    position: number;
  }
  interface E2ECuratedList {
    id: string;
    name: string;
    description: string;
    kind: string;
    accent: string;
    icon: string;
    pinned: boolean;
    position: number;
    items: E2ECuratedItem[];
  }
  interface E2ESavedView {
    id: string;
    name: string;
    description: string;
    icon: string;
    accent: string;
    query: Record<string, unknown>;
    pinned: boolean;
    position: number;
    lastUsedAt: string | null;
    useCount: number;
    createdAt: string;
    updatedAt: string;
  }
  interface E2EVideo {
    appId: number;
    videoId: string;
    provider: string;
    kind: string;
    title: string;
    channel: string;
    source: string;
    position: number;
  }

  const WISHLIST_BUCKET_IDS = ["buying_now", "waiting_sale", "considering", "watching"];
  const YOUTUBE_HOSTS = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "youtube-nocookie.com",
    "www.youtube-nocookie.com",
    "youtu.be",
  ];
  const INVALID_YOUTUBE =
    "Pega el enlace de un vídeo de YouTube (youtube.com/watch, youtu.be o /embed) o su identificador de 11 caracteres.";

  const isYoutubeId = (value: string) => /^[A-Za-z0-9_-]{11}$/.test(value);

  const parseYoutubeId = (raw: string): string => {
    const trimmed = raw.trim();
    if (!trimmed || trimmed.length > 2048) throw new Error(INVALID_YOUTUBE);
    if (isYoutubeId(trimmed)) return trimmed;
    let url: URL;
    try {
      url = new URL(trimmed.includes("://") ? trimmed : `https://${trimmed}`);
    } catch {
      throw new Error(INVALID_YOUTUBE);
    }
    if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error(INVALID_YOUTUBE);
    const host = url.hostname.toLowerCase();
    if (!YOUTUBE_HOSTS.includes(host)) throw new Error(INVALID_YOUTUBE);
    const segments = url.pathname.split("/").filter(Boolean);
    const accept = (candidate?: string) => {
      if (!candidate || !isYoutubeId(candidate)) throw new Error(INVALID_YOUTUBE);
      return candidate;
    };
    if (host === "youtu.be") return accept(segments[0]);
    if (segments[0] && ["embed", "shorts", "live", "v"].includes(segments[0])) {
      return accept(segments[1]);
    }
    if (segments[0] === "watch") return accept(url.searchParams.get("v") ?? undefined);
    throw new Error(INVALID_YOUTUBE);
  };

  let wishlist: E2EWishlistEntry[] | undefined;
  let curated: E2ECuratedList[] | undefined;
  let savedViews: E2ESavedView[] | undefined;
  const gameVideos: E2EVideo[] = [];

  const seedWishlist = (state: TestBackendState): E2EWishlistEntry[] => {
    if (wishlist) return wishlist;
    const ids = state.games.map((game) => game.appId);
    wishlist = [
      {
        appId: ids[0] ?? 1,
        bucket: "buying_now",
        priority: 5,
        position: 0,
        note: "Se compra el día uno.",
        targetPriceCents: 2999,
        currency: "EUR",
      },
      {
        appId: ids[1] ?? 2,
        bucket: "waiting_sale",
        priority: 3,
        position: 0,
        note: "",
        targetPriceCents: 1999,
        currency: "EUR",
      },
      {
        appId: ids[2] ?? 3,
        bucket: "waiting_sale",
        priority: 2,
        position: 1,
        note: "Precio de la tienda estadounidense.",
        targetPriceCents: 1500,
        currency: "USD",
      },
      { appId: ids[3] ?? 4, bucket: "considering", priority: 0, position: 0, note: "" },
    ];
    return wishlist;
  };

  const seedSavedViews = (): E2ESavedView[] => {
    if (savedViews) return savedViews;
    savedViews = [
      {
        id: "vista-corto-e-instalado",
        name: "Corto e instalado",
        description: "Lo que puedo empezar esta noche sin descargar nada.",
        icon: "bookmark",
        accent: "lime",
        query: { filters: { installed: true, maxProgress: 40 }, sort: "manual" },
        pinned: true,
        position: 0,
        lastUsedAt: "2026-08-17T21:10:00Z",
        useCount: 12,
        createdAt: "2026-06-02T09:00:00Z",
        updatedAt: "2026-08-17T21:10:00Z",
      },
      {
        id: "vista-a-medias",
        name: "A medias",
        description: "Empezados y sin terminar.",
        icon: "bookmark",
        accent: "amber",
        query: { filters: { minProgress: 10, maxProgress: 90 }, grouping: "status" },
        pinned: false,
        position: 1,
        lastUsedAt: null,
        useCount: 3,
        createdAt: "2026-06-02T09:05:00Z",
        updatedAt: "2026-07-30T18:00:00Z",
      },
      {
        id: "vista-notables",
        name: "Notables",
        description: "Nota propia de 8 para arriba.",
        icon: "bookmark",
        accent: "cyan",
        query: { filters: { minRating: 8 } },
        pinned: false,
        position: 2,
        lastUsedAt: null,
        useCount: 0,
        createdAt: "2026-06-02T09:06:00Z",
        updatedAt: "2026-06-02T09:06:00Z",
      },
    ];
    return savedViews;
  };

  const seedCurated = (state: TestBackendState): E2ECuratedList[] => {
    if (curated) return curated;
    curated = [
      {
        id: "entrada-metroidvania",
        name: "Empezar en los metroidvania",
        description: "El orden en que se los enseñaría a alguien que nunca ha jugado a uno.",
        kind: "showcase",
        accent: "lime",
        icon: "bookmark",
        pinned: true,
        position: 0,
        items: state.games.slice(0, 3).map((game, index) => ({
          appId: game.appId,
          note: index === 0 ? "El punto de partida evidente." : "",
          highlight: index === 0,
          position: index,
        })),
      },
      {
        id: "para-el-invierno",
        name: "Para el invierno",
        description: "Campañas largas para cuando no apetezca salir.",
        kind: "backlog",
        accent: "amber",
        icon: "folder",
        pinned: false,
        position: 1,
        items: [],
      },
    ];
    return curated;
  };

  const summaryOf = (state: TestBackendState, appId: number) =>
    state.games.find((game) => game.appId === appId) ?? state.games[0];

  const renumber = <T extends { position: number }>(items: T[]) => {
    items.forEach((item, index) => {
      item.position = index;
    });
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
    if (command === "list_game_dlc") {
      const appId = Number(args.appId);
      const filter = (args.filter as DlcFilter | undefined) ?? "visible";
      const matches = (item: GameDlc) => {
        if (filter === "all") return true;
        if (filter === "hidden") return item.hidden;
        if (filter === "owned") return item.owned && !item.hidden;
        if (filter === "notOwned") return !item.owned && !item.hidden;
        if (filter === "installed") return item.installed && !item.hidden;
        return !item.hidden;
      };
      return structuredClone(seedDlc(appId).filter(matches));
    }
    if (command === "get_dlc_summary") {
      return dlcSummaryOf(Number(args.appId));
    }
    if (command === "refresh_game_dlc") {
      const appId = Number(args.appId);
      const summary = dlcSummaryOf(appId);
      const unpublished = seedDlc(appId).filter((item) => item.metadataStatus === "unavailable");
      const detail = state.games.find((game) => game.appId === appId);
      // Sin instalación local no hay manifiesto: es el hueco de evidencia que la
      // ficha debe enseñar en vez de dar la ausencia por buena.
      const gap = detail?.installed ? undefined : "dlc_evidence_game_not_installed";
      return {
        appId,
        declared: summary.total,
        truncated: false,
        fetchedDetails: summary.total,
        unavailableDetails: unpublished.length,
        failedDetails: 0,
        pendingDetails: 0,
        ...(gap
          ? {
              ownershipEvidenceGap: gap,
              ownershipEvidenceExplanation:
                "El juego no está instalado en este equipo, así que no hay manifiesto local que demuestre qué DLC posees.",
            }
          : {}),
        imported: {
          appId,
          received: summary.total,
          inserted: 0,
          updated: summary.total,
          withMetadata: summary.total,
          withoutMetadata: 0,
          owned: summary.owned,
          installed: summary.installed,
        },
        summary,
      };
    }
    if (command === "set_dlc_owned") {
      return setDlcFlag(Number(args.appId), Number(args.dlcAppId), { owned: Boolean(args.owned) });
    }
    if (command === "set_dlc_installed") {
      return setDlcFlag(Number(args.appId), Number(args.dlcAppId), {
        installed: Boolean(args.installed),
      });
    }
    if (command === "set_dlc_hidden") {
      return setDlcFlag(Number(args.appId), Number(args.dlcAppId), {
        hidden: Boolean(args.hidden),
      });
    }
    if (command === "explain_priority") {
      const appId = Number(args.appId);
      const game = state.games.find((item) => item.appId === appId);
      if (!game) throw new Error("El juego ya no está en la biblioteca.");
      const locked = priorityLocks.get(appId) ?? false;
      const manualPriority = Math.min(Math.max(game.priority, 0), 5);
      // 40 (BASE_SCORE) + 24 + 20,5 − 30 − 5 + 3 = 52,5. La ficha deduce el 40 a
      // partir de estas señales, así que la aritmética que enseña cuadra.
      const score = 52.5;
      const derivedPriority = 3;
      const explanation: PriorityExplanation = {
        appId,
        title: game.title,
        score,
        effectiveScore: locked ? manualPriority * 20 : score,
        derivedPriority,
        manualPriority,
        locked,
        reason: "Tienes una partida viva a medio camino y hace semanas que no la tocas.",
        computedAt: "2026-08-18T09:00:00Z",
        manualOverride:
          locked && manualPriority !== derivedPriority
            ? `Tu prioridad manual dice ${manualPriority}; las señales dicen ${derivedPriority}. Manda la tuya.`
            : null,
        signals: [
          {
            signal: "progress_alive",
            weight: 24,
            detail: "Vas por el 60 % y esa partida sigue viva.",
          },
          {
            signal: "completed_recently",
            weight: -30,
            detail: "Lo terminaste hace poco, así que deja sitio a lo que no has cerrado.",
          },
          {
            signal: "recent_sessions",
            weight: 20.5,
            detail: "Has abierto dos sesiones en las dos últimas semanas.",
          },
          { signal: "gone_cold", weight: -5, detail: "Llevas meses sin abrirlo." },
          { signal: "pinned", weight: 3, detail: "Lo tienes fijado en la biblioteca." },
        ],
      };
      return explanation;
    }
    if (command === "set_priority_lock") {
      priorityLocks.set(Number(args.appId), Boolean(args.locked));
      return;
    }
    if (command === "recompute_priorities") {
      return {
        evaluated: state.games.length,
        updated: state.games.length,
        locked: [...priorityLocks.values()].filter(Boolean).length,
        settled: 0,
        signalsWritten: state.games.length * 5,
        highlights: [],
        computedAt: "2026-08-18T10:00:00Z",
      };
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
    if (command === "list_smart_rules") {
      const smartRules: Record<string, unknown[]> = {
        "short-sessions": [
          {
            id: "r1",
            groupId: 0,
            field: "estimatedMinutes",
            operator: "lessOrEqual",
            value: 60,
            position: 0,
          },
        ],
        "unfinished-stories": [
          {
            id: "r2",
            groupId: 0,
            field: "progress",
            operator: "greaterOrEqual",
            value: 20,
            position: 0,
          },
          {
            id: "r3",
            groupId: 0,
            field: "progress",
            operator: "lessOrEqual",
            value: 80,
            position: 1,
          },
        ],
        "drm-free": [
          {
            id: "r4",
            groupId: 0,
            field: "category",
            operator: "contains",
            value: "DRM-Free",
            position: 0,
          },
        ],
      };
      return smartRules[String(args.collectionId)] ?? [];
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
      const request = (args.request ?? {}) as {
        query?: string;
        availability?: string;
        sort?: string;
        limit?: number;
        offset?: number;
      };
      const query = request.query?.trim().toLocaleLowerCase("es-ES") ?? "";
      const items = familyCatalog
        .filter(
          (game) =>
            (!query || game.title.toLocaleLowerCase("es-ES").includes(query)) &&
            (!request.availability || game.availability === request.availability),
        )
        .sort((left, right) => {
          if (request.sort === "alphabeticalDesc") return right.title.localeCompare(left.title);
          if (request.sort === "updatedDesc") return right.updatedAt.localeCompare(left.updatedAt);
          if (request.sort === "discoveredDesc") {
            return right.discoveredAt.localeCompare(left.discoveredAt);
          }
          if (request.sort === "availability" || !request.sort) {
            const availability =
              Number(right.availability === "confirmed") -
              Number(left.availability === "confirmed");
            if (availability) return availability;
          }
          return left.title.localeCompare(right.title);
        });
      const offset = request.offset ?? 0;
      const limit = request.limit ?? 240;
      return {
        items: items.slice(offset, offset + limit),
        total: items.length,
        limit,
        offset,
      };
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
    if (command === "get_notification_inbox") {
      // La vitrina siembra avisos reales para poder juzgar la bandeja llena.
      if (state.scenario !== "showcase") {
        return {
          items: [],
          total: 0,
          limit: 0,
          offset: 0,
          unread: { total: 0, info: 0, success: 0, warning: 0, critical: 0 },
        };
      }
      const pick = (index: number) => state.games[index % state.games.length];
      const seeds = [
        {
          kind: "early_access_exit",
          severity: "success",
          title: "Hades II ha salido de acceso anticipado",
          body: "La versión 1.0 ya está disponible. Tu partida guardada sigue siendo válida.",
          index: 27,
          occurredAt: "2026-08-17T18:20:00Z",
        },
        {
          kind: "release_date_changed",
          severity: "info",
          title: "Cambio de fecha de lanzamiento",
          body: "La fecha pasó del 30 de septiembre al 14 de noviembre de 2026.",
          index: 17,
          occurredAt: "2026-08-17T11:05:00Z",
        },
        {
          kind: "reminder_due",
          severity: "warning",
          title: "Recordatorio vencido",
          body: "Retomar la ruta del observatorio antes de que se olvide el mapa.",
          index: 0,
          occurredAt: "2026-08-16T18:00:00Z",
        },
        {
          kind: "official_news",
          severity: "info",
          title: "Actualización 1.14: reequilibrio de armas y correcciones",
          body: "Publicado por el estudio en el canal oficial de anuncios.",
          index: 3,
          occurredAt: "2026-08-15T09:40:00Z",
        },
        {
          kind: "dlc_release",
          severity: "info",
          title: "Contenido adicional nuevo detectado",
          body: "Se ha publicado un DLC para un juego de tu biblioteca.",
          index: 12,
          occurredAt: "2026-08-14T20:10:00Z",
        },
      ];
      const items = seeds.map((seed, position) => {
        const game = pick(seed.index);
        return {
          id: `event-${position}`,
          appId: game?.appId,
          gameTitle: game?.title,
          kind: seed.kind,
          severity: seed.severity,
          title: seed.title,
          body: seed.body,
          occurredAt: seed.occurredAt,
          readAt: position > 2 ? "2026-08-17T22:00:00Z" : undefined,
        };
      });
      return {
        items,
        total: items.length,
        limit: 40,
        offset: 0,
        unread: { total: 3, info: 1, success: 1, warning: 1, critical: 0 },
      };
    }
    if (command === "refresh_notification_events") {
      return {
        scheduledEvents: 0,
        derived: {
          earlyAccessExits: 1,
          releaseDateChanges: 1,
          officialNews: 1,
          dueReminders: 1,
          newDlc: 1,
          created: 5,
          skippedDuplicates: 0,
        },
        unread: { total: 3, info: 1, success: 1, warning: 1, critical: 0 },
      };
    }
    if (
      [
        "mark_notification_read",
        "mark_all_notifications_read",
        "dismiss_notification",
        "dismiss_all_notifications",
      ].includes(command)
    ) {
      return 0;
    }
    if (command === "list_notification_rules") {
      seedNotifications(state);
      const appId = args.appId as number | undefined;
      return [...notificationRules.values()]
        .filter((rule) => appId === undefined || rule.appId === appId)
        .map(decorateRule)
        .sort((left, right) =>
          (left.nextOccurrence ?? "\uffff").localeCompare(right.nextOccurrence ?? "\uffff"),
        );
    }
    if (command === "save_notification_rule") {
      const input = args.input as SaveNotificationRuleInput;
      const title = input.title.trim();
      // Mismos rechazos que `validate_rule_input` en Rust: la interfaz tiene
      // que poder enseñar el error real, no uno inventado por el arnés.
      if (!title) throw new Error("El aviso necesita un título: escribe qué quieres recordar.");
      const needsGame = ["release_date", "early_access_exit", "official_news", "dlc_release"];
      if (needsGame.includes(input.kind) && !input.appId) {
        throw new Error(
          "Este tipo de aviso habla de un juego concreto: elige el juego antes de guardarlo.",
        );
      }
      if (!input.scheduledFor && input.kind === "manual") {
        throw new Error(
          "Un aviso manual sin fecha no puede dispararse: indica cuándo quieres recibirlo.",
        );
      }
      const stamp = new Date().toISOString();
      const id = input.id ?? `rule-${notificationRules.size + 1}-${Date.now()}`;
      const previous = notificationRules.get(id);
      const game = input.appId
        ? state.games.find((candidate) => candidate.appId === input.appId)
        : undefined;
      const saved: NotificationRule = {
        id,
        ...(input.appId ? { appId: input.appId } : {}),
        ...(game ? { gameTitle: game.title } : {}),
        kind: input.kind,
        title,
        body: input.body?.trim() ?? "",
        ...(input.scheduledFor ? { scheduledFor: input.scheduledFor } : {}),
        repeatRule: input.repeatRule,
        leadMinutes: input.leadMinutes ?? 0,
        enabled: input.enabled,
        ...(previous?.lastFiredAt ? { lastFiredAt: previous.lastFiredAt } : {}),
        createdAt: previous?.createdAt ?? stamp,
        updatedAt: stamp,
      };
      notificationRules.set(id, saved);
      return decorateRule(saved);
    }
    if (command === "delete_notification_rule") {
      if (!notificationRules.delete(String(args.id))) {
        throw new Error("El aviso programado ya no existe.");
      }
      return;
    }
    if (command === "list_upcoming_releases") {
      seedNotifications(state);
      const limit = Number(args.limit ?? 40);
      return [...upcomingReleases.values()]
        .filter((item) => item.dismissedAt === null)
        .sort(
          (left, right) =>
            right.matchScore - left.matchScore ||
            (left.releaseDate ?? "\uffff").localeCompare(right.releaseDate ?? "\uffff"),
        )
        .slice(0, limit)
        .map((item) => ({ ...item }));
    }
    if (command === "dismiss_upcoming_release") {
      seedNotifications(state);
      const item = upcomingReleases.get(Number(args.appId));
      if (!item) throw new Error("Ese lanzamiento ya no está entre los candidatos.");
      item.dismissedAt = item.dismissedAt ?? new Date().toISOString();
      return;
    }
    if (command === "record_taste_feedback") {
      seedNotifications(state);
      const item = upcomingReleases.get(Number(args.appId));
      // Igual que en Rust: «me interesa» devuelve el candidato a la lista y los
      // otros dos veredictos lo retiran.
      if (item) {
        item.dismissedAt =
          args.verdict === "interested" ? null : (item.dismissedAt ?? new Date().toISOString());
      }
      return;
    }
    if (command === "score_upcoming_releases") {
      seedNotifications(state);
      return [...upcomingReleases.values()].filter((item) => item.dismissedAt === null).length;
    }
    if (command === "learn_taste") {
      seedNotifications(state);
      const report: TasteReport = {
        gamesAnalyzed: state.games.length,
        dismissedUpcomingUsed: [...upcomingReleases.values()].filter((item) => item.dismissedAt)
          .length,
        facetsLearned: 3,
        positiveFacets: 2,
        negativeFacets: 1,
        highlights: [
          {
            facet: "genre",
            facetLabel: "Género",
            value: "Metroidvania",
            weight: 0.72,
            positiveSamples: 14,
            negativeSamples: 1,
          },
          {
            facet: "developer",
            facetLabel: "Desarrollador",
            value: "Team Cherry",
            weight: 0.58,
            positiveSamples: 3,
            negativeSamples: 0,
          },
          {
            facet: "category",
            facetLabel: "Categoría",
            value: "Multijugador masivo",
            weight: -0.31,
            positiveSamples: 0,
            negativeSamples: 9,
          },
        ],
        computedAt: new Date().toISOString(),
      };
      return report;
    }
    if (command === "get_discovery_snapshot") {
      // El escenario de vitrina siembra señales realistas para que la revisión
      // visual juzgue la pantalla llena y no su estado vacío. El resto de
      // escenarios conserva el vacío, que es lo que verifican sus pruebas.
      if (state.scenario !== "showcase") {
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
      const pick = (index: number) => state.games[index % state.games.length];
      const summary = (index: number) => {
        const item = pick(index);
        return item ? { ...item } : undefined;
      };
      return {
        reminders: [0, 4, 8].map((index, position) => {
          const item = pick(index);
          return {
            id: `reminder-${index}`,
            appId: item?.appId ?? 0,
            title: item?.title ?? "",
            iconUrl: item?.coverUrl,
            dueAt: `2026-08-${19 + position}T18:00:00Z`,
            note: [
              "Retomar la ruta del observatorio antes de que se olvide el mapa.",
              "Comprobar si la actualización de temporada cambia el equilibrio.",
              "Terminar el último jefe pendiente de esta partida.",
            ][position],
          };
        }),
        forgotten: [10, 13, 20, 24].map((index) => summary(index)).filter(Boolean),
        almostFinished: [2, 5, 11].map((index) => summary(index)).filter(Boolean),
        upcoming: [17, 27, 39].map((index) => summary(index)).filter(Boolean),
        events: [1, 7].map((index, position) => {
          const item = pick(index);
          return {
            id: `event-${index}`,
            appId: item?.appId ?? 0,
            title: item?.title ?? "",
            iconUrl: item?.coverUrl,
            kind: position === 0 ? "early_access_changed" : "release_date_changed",
            previousValue: position === 0 ? "true" : "2026-09-30",
            currentValue: position === 0 ? "false" : "2026-11-14",
            observedAt: `2026-08-1${5 + position}T09:30:00Z`,
          };
        }),
        officialPublications: [0, 3, 6, 12].map((index, position) => {
          const item = pick(index);
          return {
            appId: item?.appId ?? 0,
            gameTitle: item?.title ?? "",
            iconUrl: item?.coverUrl,
            gid: `news-${index}`,
            title: [
              "Actualización 1.14: reequilibrio de armas y correcciones",
              "Notas de la temporada: nuevo mapa y modo cooperativo",
              "Parche de rendimiento para equipos con GPU integrada",
              "Aniversario: evento temporal y recompensas",
            ][position],
            contentPreview:
              "Resumen oficial publicado por el estudio en el canal de anuncios de la comunidad.",
            publishedAt: `2026-08-1${4 + position}T11:00:00Z`,
            feedLabel: "Anuncios de la comunidad",
          };
        }),
        relatedReleases: [9, 15, 22].map((index, position) => {
          const item = pick(index);
          const related = pick(index + 1);
          return {
            appId: item?.appId ?? 0,
            title: item?.title ?? "",
            iconUrl: item?.coverUrl,
            releaseDate: `2026-1${position}-08`,
            criterion: position % 2 === 0 ? "developer" : "publisher",
            criterionValue:
              (position % 2 === 0 ? item?.developer : item?.publisher) ?? "Estudio verificado",
            relatedToAppId: related?.appId ?? 0,
            relatedToTitle: related?.title ?? "",
          };
        }),
        dismissedRecommendations: [
          {
            id: "history-1",
            appId: pick(30)?.appId ?? 0,
            title: pick(30)?.title ?? "",
            iconUrl: pick(30)?.coverUrl,
            createdAt: "2026-08-12T21:00:00Z",
          },
        ],
        capabilities: {
          metadataObservations: 48,
          earlyAccessHistoryAvailable: true,
          trackedNewsGames: 11,
          officialPublicationsAvailable: true,
          relatedReleasesAvailable: true,
        },
      };
    }
    if (command === "recommend_game") {
      if (state.scenario !== "showcase") throw new Error("Sin recomendación disponible.");
      const candidate = state.games.find((item) => item.statusId === "playing") ?? state.games[0];
      if (!candidate) throw new Error("Sin recomendación disponible.");
      return {
        historyId: "recommendation-1",
        game: { ...candidate },
        reasons: [
          "Lo tienes a medias: 62 % de progreso y sesión reciente.",
          "Encaja en una hora según tus últimas sesiones.",
          "Está instalado y listo para abrir.",
        ],
      };
    }
    if (
      [
        "save_reminder",
        "complete_reminder",
        "snooze_reminder",
        "dismiss_recommendation",
        "restore_recommendation",
      ].includes(command)
    ) {
      return;
    }
    if (command === "refresh_discovery_news") {
      return {
        attemptedGames: 11,
        refreshedGames: 11,
        publicationsSaved: 24,
        failedGames: 0,
      };
    }
    // ── Deseados ───────────────────────────────────────────────────────────
    if (command === "get_wishlist_overview") {
      const entries = seedWishlist(state);
      const buckets = WISHLIST_BUCKET_IDS.map((bucket) => {
        const items = entries
          .filter((entry) => entry.bucket === bucket)
          .sort((left, right) => left.position - right.position)
          .map((entry) => ({
            game: { ...summaryOf(state, entry.appId) },
            bucket: entry.bucket,
            priority: entry.priority,
            position: entry.position,
            note: entry.note,
            targetPriceCents: entry.targetPriceCents,
            currency: entry.currency,
            addedAt: "2026-08-01T10:00:00Z",
            updatedAt: "2026-08-14T10:00:00Z",
          }));
        return { bucket, items, total: items.length };
      });
      const totals = new Map<string, { totalCents: number; entries: number }>();
      for (const entry of entries) {
        if (entry.targetPriceCents === undefined) continue;
        const code = entry.currency ?? "EUR";
        const current = totals.get(code) ?? { totalCents: 0, entries: 0 };
        totals.set(code, {
          totalCents: current.totalCents + entry.targetPriceCents,
          entries: current.entries + 1,
        });
      }
      return {
        buckets,
        total: entries.length,
        targetTotals: [...totals].map(([currency, value]) => ({ currency, ...value })),
        entriesWithoutTarget: entries.filter((entry) => entry.targetPriceCents === undefined)
          .length,
      };
    }
    if (command === "save_wishlist_entry") {
      const entries = seedWishlist(state);
      const input = args.input as {
        appId: number;
        bucket: string;
        priority: number;
        note: string;
        targetPriceCents?: number;
        currency?: string;
      };
      if (!WISHLIST_BUCKET_IDS.includes(input.bucket)) {
        throw new Error("El cubo de deseados no es válido.");
      }
      const existing = entries.find((entry) => entry.appId === input.appId);
      const stored: E2EWishlistEntry = existing ?? {
        appId: input.appId,
        bucket: input.bucket,
        priority: 0,
        position: entries.filter((entry) => entry.bucket === input.bucket).length,
        note: "",
      };
      stored.bucket = input.bucket;
      stored.priority = Math.min(5, Math.max(0, Math.trunc(input.priority)));
      stored.note = input.note ?? "";
      stored.targetPriceCents = input.targetPriceCents;
      stored.currency = input.targetPriceCents === undefined ? undefined : input.currency;
      if (!existing) entries.push(stored);
      return {
        game: { ...summaryOf(state, stored.appId) },
        bucket: stored.bucket,
        priority: stored.priority,
        position: stored.position,
        note: stored.note,
        targetPriceCents: stored.targetPriceCents,
        currency: stored.currency,
        addedAt: "2026-08-01T10:00:00Z",
        updatedAt: "2026-08-14T10:00:00Z",
      };
    }
    if (command === "remove_wishlist_entry") {
      const entries = seedWishlist(state);
      const index = entries.findIndex((entry) => entry.appId === Number(args.appId));
      if (index >= 0) entries.splice(index, 1);
      return;
    }
    if (command === "move_wishlist_entry") {
      const entries = seedWishlist(state);
      const bucket = String(args.bucket);
      if (!WISHLIST_BUCKET_IDS.includes(bucket)) {
        throw new Error("El cubo de deseados no es válido.");
      }
      const moving = entries.find((entry) => entry.appId === Number(args.appId));
      if (!moving) throw new Error("Ese juego no está en los deseados.");
      moving.bucket = bucket;
      const lane = entries
        .filter((entry) => entry.bucket === bucket && entry.appId !== moving.appId)
        .sort((left, right) => left.position - right.position);
      const anchor =
        args.beforeAppId === undefined || args.beforeAppId === null
          ? -1
          : lane.findIndex((entry) => entry.appId === Number(args.beforeAppId));
      if (anchor < 0) lane.push(moving);
      else lane.splice(anchor, 0, moving);
      renumber(lane);
      return;
    }
    if (command === "reorder_wishlist_bucket") {
      const entries = seedWishlist(state);
      const bucket = String(args.bucket);
      const ordered = (args.orderedAppIds as number[]) ?? [];
      ordered.forEach((appId, index) => {
        const entry = entries.find((candidate) => candidate.appId === appId);
        if (!entry) return;
        entry.bucket = bucket;
        entry.position = index;
      });
      return;
    }

    // ── Precios de la lista de deseados ────────────────────────────────────
    // Se derivan de las entradas sembradas para que la pantalla de deseados se
    // pueda revisar con precios de verdad: uno por debajo del objetivo, uno con
    // descuento pero todavía por encima, uno en otra moneda que por tanto no es
    // comparable, y uno sin precio observado. Cada rama de la interfaz tiene su
    // caso, y ninguno finge un importe que la tienda no habría dado.
    if (command === "list_wishlist_prices") {
      const observado = "2026-08-17T09:00:00Z";
      const precio = (
        appId: number,
        currency: string,
        finalCents: number,
        initialCents: number,
      ) => ({
        appId,
        currency,
        countryCode: currency === "EUR" ? "ES" : "US",
        finalCents,
        initialCents,
        discountPercent:
          initialCents > finalCents
            ? Math.round(((initialCents - finalCents) / initialCents) * 100)
            : 0,
        lowestCents: finalCents,
        lowestObservedAt: observado,
        changedAt: observado,
        observedAt: observado,
        source: "steam_store" as const,
        freshness: "fresh" as const,
        ageMinutes: 45,
      });

      return seedWishlist(state).map((entry, indice) => {
        // La cuarta entrada se queda sin precio observado a propósito.
        if (indice === 3) {
          return {
            appId: entry.appId,
            otherCurrencies: [],
            comparable: false,
            meetsTarget: false,
          };
        }
        const moneda = entry.currency ?? "EUR";
        const objetivo = entry.targetPriceCents;
        // La primera cumple el objetivo; la segunda está de oferta pero por
        // encima; la tercera está en otra moneda que la del objetivo.
        const finales = [1999, 2399, 1799];
        const iniciales = [3999, 4999, 1799];
        const gamePrice = precio(entry.appId, moneda, finales[indice], iniciales[indice]);
        const comparable = objetivo !== undefined;
        const differenceCents = comparable ? gamePrice.finalCents - objetivo : undefined;
        return {
          appId: entry.appId,
          targetCents: objetivo,
          targetCurrency: objetivo === undefined ? undefined : moneda,
          price: gamePrice,
          otherCurrencies: [],
          comparable,
          differenceCents,
          meetsTarget: differenceCents !== undefined && differenceCents <= 0,
        };
      });
    }

    // ── Vídeos por juego ───────────────────────────────────────────────────
    if (command === "list_game_videos") {
      const appId = Number(args.appId);
      const kind = args.kind === undefined || args.kind === null ? undefined : String(args.kind);
      return gameVideos
        .filter((video) => video.appId === appId && (!kind || video.kind === kind))
        .sort((left, right) => left.position - right.position)
        .map((video) => ({
          ...video,
          createdAt: "2026-08-14T10:00:00Z",
          // La construye el backend, nunca la pantalla.
          embedUrl:
            video.provider === "youtube"
              ? `https://www.youtube-nocookie.com/embed/${video.videoId}`
              : undefined,
        }));
    }
    if (command === "save_game_video") {
      const input = args.input as {
        appId: number;
        video: string;
        provider?: string;
        kind?: string;
        title?: string;
        channel?: string;
        source?: string;
      };
      const provider = input.provider ?? "youtube";
      if (provider !== "youtube") throw new Error("El proveedor de vídeo no es válido.");
      const videoId = parseYoutubeId(input.video);
      const kind = input.kind ?? "gameplay";
      const existing = gameVideos.find(
        (video) =>
          video.appId === input.appId && video.provider === provider && video.videoId === videoId,
      );
      const stored: E2EVideo = existing ?? {
        appId: input.appId,
        videoId,
        provider,
        kind,
        title: "",
        channel: "",
        source: input.source ?? "manual",
        position: gameVideos.filter((video) => video.appId === input.appId && video.kind === kind)
          .length,
      };
      stored.kind = kind;
      stored.title = input.title ?? stored.title;
      stored.channel = input.channel ?? stored.channel;
      if (!existing) gameVideos.push(stored);
      return {
        ...stored,
        createdAt: "2026-08-14T10:00:00Z",
        embedUrl: `https://www.youtube-nocookie.com/embed/${stored.videoId}`,
      };
    }
    if (command === "delete_game_video") {
      const index = gameVideos.findIndex(
        (video) =>
          video.appId === Number(args.appId) &&
          video.provider === String(args.provider) &&
          video.videoId === String(args.videoId),
      );
      if (index >= 0) gameVideos.splice(index, 1);
      return;
    }
    if (command === "reorder_game_videos") {
      const appId = Number(args.appId);
      const kind = String(args.kind);
      const ordered = (args.ordered as { provider: string; videoId: string }[]) ?? [];
      ordered.forEach((reference, index) => {
        const video = gameVideos.find(
          (candidate) =>
            candidate.appId === appId &&
            candidate.kind === kind &&
            candidate.provider === reference.provider &&
            candidate.videoId === reference.videoId,
        );
        if (video) video.position = index;
      });
      return;
    }

    // ── Vistas guardadas ───────────────────────────────────────────────────
    if (command === "list_saved_views") {
      return seedSavedViews()
        .slice()
        .sort((left, right) => {
          if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
          return left.position - right.position;
        });
    }
    if (command === "save_saved_view") {
      const views = seedSavedViews();
      const input = args.input as {
        id?: string;
        name: string;
        description?: string;
        icon?: string;
        accent?: string;
        query: Record<string, unknown>;
        pinned?: boolean;
      };
      const name = input.name.trim();
      if (!name) throw new Error("La vista necesita un nombre.");
      const clash = views.find(
        (view) => view.name.toLowerCase() === name.toLowerCase() && view.id !== input.id,
      );
      if (clash) throw new Error("Ya existe una vista con ese nombre. Elige otro.");
      const existing = input.id ? views.find((view) => view.id === input.id) : undefined;
      const stored: E2ESavedView = existing ?? {
        id: `vista-${views.length + 1}`,
        name,
        description: "",
        icon: "bookmark",
        accent: "cyan",
        query: {},
        pinned: false,
        position: views.length,
        lastUsedAt: null,
        useCount: 0,
        createdAt: "2026-08-18T10:00:00Z",
        updatedAt: "2026-08-18T10:00:00Z",
      };
      Object.assign(stored, {
        name,
        description: input.description ?? stored.description,
        icon: input.icon || "bookmark",
        accent: input.accent || "cyan",
        query: input.query,
        pinned: input.pinned ?? false,
        updatedAt: "2026-08-18T10:00:00Z",
      });
      if (!existing) views.push(stored);
      return stored;
    }
    if (command === "delete_saved_view") {
      const views = seedSavedViews();
      const index = views.findIndex((view) => view.id === args.viewId);
      if (index < 0) throw new Error("Esa vista guardada ya no existe.");
      views.splice(index, 1);
      renumber(views);
      return null;
    }
    if (command === "reorder_saved_views") {
      const views = seedSavedViews();
      const order = args.orderedIds as string[];
      views.sort((left, right) => order.indexOf(left.id) - order.indexOf(right.id));
      renumber(views);
      return null;
    }
    if (command === "mark_saved_view_used") {
      const view = seedSavedViews().find((entry) => entry.id === args.viewId);
      if (!view) throw new Error("Esa vista guardada ya no existe.");
      view.useCount += 1;
      view.lastUsedAt = "2026-08-18T10:05:00Z";
      return view;
    }

    // ── Listas curadas ─────────────────────────────────────────────────────
    if (command === "list_curated_lists") {
      return seedCurated(state)
        .slice()
        .sort((left, right) => left.position - right.position)
        .map((list) => ({
          id: list.id,
          name: list.name,
          description: list.description,
          kind: list.kind,
          accent: list.accent,
          icon: list.icon,
          pinned: list.pinned,
          position: list.position,
          gameCount: list.items.length,
          createdAt: "2026-08-01T10:00:00Z",
          updatedAt: "2026-08-14T10:00:00Z",
        }));
    }
    if (command === "save_curated_list") {
      const lists = seedCurated(state);
      const input = args.input as {
        id?: string;
        name: string;
        description: string;
        kind: string;
        accent: string;
        icon: string;
        pinned: boolean;
      };
      if (!input.name.trim()) throw new Error("La lista necesita un nombre.");
      const existing = input.id ? lists.find((list) => list.id === input.id) : undefined;
      const stored: E2ECuratedList = existing ?? {
        id: `list-${lists.length + 1}`,
        name: input.name,
        description: input.description,
        kind: input.kind,
        accent: input.accent,
        icon: input.icon,
        pinned: input.pinned,
        position: lists.length,
        items: [],
      };
      Object.assign(stored, {
        name: input.name.trim(),
        description: input.description,
        kind: input.kind,
        accent: input.accent,
        icon: input.icon,
        pinned: input.pinned,
      });
      if (!existing) lists.push(stored);
      return {
        id: stored.id,
        name: stored.name,
        description: stored.description,
        kind: stored.kind,
        accent: stored.accent,
        icon: stored.icon,
        pinned: stored.pinned,
        position: stored.position,
        gameCount: stored.items.length,
        createdAt: "2026-08-01T10:00:00Z",
        updatedAt: "2026-08-14T10:00:00Z",
      };
    }
    if (command === "delete_curated_list") {
      const lists = seedCurated(state);
      const index = lists.findIndex((list) => list.id === String(args.listId));
      if (index >= 0) lists.splice(index, 1);
      renumber(lists);
      return;
    }
    if (command === "reorder_curated_lists") {
      const lists = seedCurated(state);
      ((args.orderedIds as string[]) ?? []).forEach((id, index) => {
        const list = lists.find((candidate) => candidate.id === id);
        if (list) list.position = index;
      });
      return;
    }
    if (command === "get_curated_list_detail") {
      const lists = seedCurated(state);
      const list = lists.find((candidate) => candidate.id === String(args.listId));
      if (!list) throw new Error("Esa lista curada ya no existe.");
      const items = list.items
        .slice()
        .sort((left, right) => left.position - right.position)
        .map((item) => ({
          game: { ...summaryOf(state, item.appId) },
          note: item.note,
          highlight: item.highlight,
          position: item.position,
          addedAt: "2026-08-01T10:00:00Z",
        }));
      return {
        list: {
          id: list.id,
          name: list.name,
          description: list.description,
          kind: list.kind,
          accent: list.accent,
          icon: list.icon,
          pinned: list.pinned,
          position: list.position,
          gameCount: list.items.length,
          createdAt: "2026-08-01T10:00:00Z",
          updatedAt: "2026-08-14T10:00:00Z",
        },
        items,
        total: items.length,
        limit: Number(args.limit ?? 60),
        offset: Number(args.offset ?? 0),
      };
    }
    if (command === "add_curated_game") {
      const lists = seedCurated(state);
      const input = args.input as {
        listId: string;
        appId: number;
        note: string;
        highlight: boolean;
        beforeAppId?: number;
      };
      const list = lists.find((candidate) => candidate.id === input.listId);
      if (!list) throw new Error("Esa lista curada ya no existe.");
      if (list.items.some((item) => item.appId === input.appId)) return;
      const item: E2ECuratedItem = {
        appId: input.appId,
        note: input.note ?? "",
        highlight: Boolean(input.highlight),
        position: list.items.length,
      };
      const anchor =
        input.beforeAppId === undefined
          ? -1
          : list.items.findIndex((candidate) => candidate.appId === input.beforeAppId);
      if (anchor < 0) list.items.push(item);
      else list.items.splice(anchor, 0, item);
      renumber(list.items);
      return;
    }
    if (command === "update_curated_item") {
      const lists = seedCurated(state);
      const input = args.input as {
        listId: string;
        appId: number;
        note: string;
        highlight: boolean;
      };
      const item = lists
        .find((list) => list.id === input.listId)
        ?.items.find((candidate) => candidate.appId === input.appId);
      if (!item) throw new Error("Esa entrada ya no está en la lista.");
      item.note = input.note ?? "";
      item.highlight = Boolean(input.highlight);
      return;
    }
    if (command === "remove_curated_game") {
      const list = seedCurated(state).find((candidate) => candidate.id === String(args.listId));
      if (!list) return;
      const index = list.items.findIndex((item) => item.appId === Number(args.appId));
      if (index >= 0) list.items.splice(index, 1);
      renumber(list.items);
      return;
    }
    if (command === "move_curated_item") {
      const list = seedCurated(state).find((candidate) => candidate.id === String(args.listId));
      if (!list) return;
      const index = list.items.findIndex((item) => item.appId === Number(args.appId));
      if (index < 0) return;
      const [moving] = list.items.splice(index, 1);
      if (!moving) return;
      const anchor =
        args.beforeAppId === undefined || args.beforeAppId === null
          ? -1
          : list.items.findIndex((item) => item.appId === Number(args.beforeAppId));
      if (anchor < 0) list.items.push(moving);
      else list.items.splice(anchor, 0, moving);
      renumber(list.items);
      return;
    }
    if (command === "reorder_curated_items") {
      const list = seedCurated(state).find((candidate) => candidate.id === String(args.listId));
      if (!list) return;
      const ordered = (args.orderedAppIds as number[]) ?? [];
      list.items.sort((left, right) => {
        const leftIndex = ordered.indexOf(left.appId);
        const rightIndex = ordered.indexOf(right.appId);
        return (
          (leftIndex < 0 ? ordered.length : leftIndex) -
          (rightIndex < 0 ? ordered.length : rightIndex)
        );
      });
      renumber(list.items);
      return;
    }

    if (command === "cache_game_art") {
      // El escenario de vitrina sirve el arte oficial tal cual: `convertFileSrc`
      // devuelve la ruta sin transformar en este arnés, de modo que la imagen
      // llega a la CDN pública y las capturas reflejan la aplicación real.
      if (state.scenario === "showcase") {
        const source = (args as { sourceUrl?: string }).sourceUrl;
        if (typeof source === "string" && source.startsWith("https://")) {
          const variant = String((args as { variant?: string }).variant ?? "cover");
          // Tamaños reales de la CDN oficial, para que la reserva de hueco de
          // la interfaz se comporte igual que en la aplicación empaquetada.
          const size = variant.startsWith("cover")
            ? { width: 600, height: 900 }
            : variant.startsWith("hero")
              ? { width: 3840, height: 1240 }
              : variant.startsWith("header")
                ? { width: 460, height: 215 }
                : { width: 184, height: 69 };
          return {
            appId: Number((args as { appId?: number }).appId ?? 0),
            variant,
            localPath: source,
            width: size.width,
            height: size.height,
            bytes: 0,
          };
        }
      }
      throw new Error("E2E_ARTWORK_OFFLINE");
    }
    // ── Catálogo de Steam Family por sesión ────────────────────────────────
    // El vínculo se simula ya hecho y con una lectura buena detrás: es el
    // estado que hay que poder revisar, porque el de «sin vincular» no enseña
    // ni el recuento ni los botones de sincronizar y olvidar.
    if (command === "steam_family_session_status") {
      return {
        linked: true,
        lastSyncAt: "2026-08-17T09:30:00Z",
        lastAppCount: 3812,
      };
    }
    if (command === "link_steam_family_session") {
      return { linked: true };
    }
    if (command === "unlink_steam_family_session") {
      return { linked: false };
    }
    if (command === "sync_steam_family_catalog") {
      return { imported: 3812, unusable: 0, withoutTitle: 4, noFamily: false };
    }
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
