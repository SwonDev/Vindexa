import type {
  AppBootstrap,
  CollectionSummary,
  GameDetail,
  GameSummary,
  PlannerOverview,
  StatusDefinition,
} from "../../../src/lib/types";
import {
  SHOWCASE_CATALOG,
  type ShowcaseEntry,
  showcaseCoverUrl,
  showcaseHeaderUrl,
  showcaseHeroUrl,
} from "./showcase-catalog";

export type VindexaScenario =
  | "empty"
  | "library"
  | "library-error"
  | "database-recovery"
  | "startup-error"
  | "startup-recovery"
  | "showcase"
  | "scale";

export interface TestBackendState {
  scenario: VindexaScenario;
  bootstrap: AppBootstrap;
  games: GameDetail[];
  collections: Record<string, number[]>;
  planner: PlannerOverview;
  bootstrapFailuresRemaining: number;
  commandLog: string[];
}

const statuses: StatusDefinition[] = [
  {
    id: "unclassified",
    name: "Sin clasificar",
    color: "#8493A4",
    position: 0,
    builtIn: true,
    gameCount: 0,
  },
  {
    id: "playing",
    name: "Jugando",
    color: "#5CAAC1",
    position: 1,
    builtIn: true,
    gameCount: 0,
  },
  {
    id: "finished",
    name: "Terminados",
    color: "#7EA64B",
    position: 2,
    builtIn: true,
    gameCount: 0,
  },
];

const collections: CollectionSummary[] = [
  {
    id: "favorites",
    name: "Favoritos",
    description: "Los mundos a los que siempre merece la pena volver.",
    color: "#D6A64B",
    icon: "star",
    kind: "manual",
    matchMode: "all",
    position: 0,
    gameCount: 1,
  },
  {
    id: "short-sessions",
    name: "Sesiones cortas",
    description: "Selección inteligente para una hora libre.",
    color: "#5CAAC1",
    icon: "bolt",
    kind: "smart",
    matchMode: "all",
    position: 1,
    gameCount: 2,
  },
];

function game(overrides: Partial<GameDetail> & Pick<GameDetail, "appId" | "title">): GameDetail {
  return {
    appId: overrides.appId,
    title: overrides.title,
    playtimeMinutes: 1_860,
    playtimeRecentMinutes: 180,
    lastPlayedAt: "2026-08-13T20:15:00Z",
    releaseDate: "2024-03-21",
    isEarlyAccess: false,
    isFree: false,
    steamDeckStatus: "verified",
    achievementsUnlocked: 18,
    achievementsTotal: 42,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: true,
    installPath: "/Volumes/TestLibrary/steamapps/common/VindexaFixture",
    sizeOnDisk: 12_884_901_888,
    statusId: "playing",
    statusName: "Jugando",
    statusColor: "#5CAAC1",
    progress: 48,
    priority: 4,
    pinned: true,
    tracking: true,
    rating: 9,
    estimatedMinutes: 540,
    targetDate: "2026-08-31",
    nextAction: "Abrir la puerta del observatorio",
    checkpoint: "Campamento del lago, antes de activar el observatorio.",
    notes: "Explorar la ruta norte antes de continuar la historia.",
    manualPosition: 0,
    shortDescription:
      "Una aventura de exploración precisa y atmosférica donde cada ruta descubierta transforma tu mapa personal.",
    developer: "Northstar Workshop",
    publisher: "Northstar Workshop",
    genres: ["Aventura", "Exploración"],
    categories: ["Un jugador", "Compatibilidad con mando"],
    metadataStatus: "success",
    metadataFetchedAt: "2026-08-14T08:00:00Z",
    achievementsStatus: "success",
    achievementsFetchedAt: "2026-08-14T08:00:00Z",
    collectionIds: ["favorites"],
    tags: ["Inmersivo", "Fin de semana"],
    tagIds: [],
    sessions: [],
    sessionsTotal: 0,
    activity: [
      {
        id: `activity-${overrides.appId}`,
        kind: "progress",
        message: "Progreso actualizado al 48%",
        createdAt: "2026-08-13T20:15:00Z",
      },
    ],
    ...overrides,
  };
}

const games = [
  game({ appId: 101, title: "Nebula Frontier", manualPosition: 0 }),
  game({
    appId: 202,
    title: "Clockwork Harbor",
    statusId: "unclassified",
    statusName: "Sin clasificar",
    statusColor: "#8493A4",
    progress: 12,
    priority: 2,
    pinned: false,
    rating: 7,
    playtimeMinutes: 320,
    manualPosition: 1,
    collectionIds: [],
  }),
  game({
    appId: 303,
    title: "Mosslight Valley",
    statusId: "finished",
    statusName: "Terminados",
    statusColor: "#7EA64B",
    progress: 100,
    priority: 1,
    pinned: false,
    rating: 8,
    playtimeMinutes: 4_820,
    installed: false,
    installPath: undefined,
    sizeOnDisk: undefined,
    manualPosition: 2,
    collectionIds: [],
  }),
];

function plannerFor(items: GameSummary[]): PlannerOverview {
  const [primary] = items;
  return {
    columns: [
      {
        id: "this-week",
        name: "Esta semana",
        color: "#5CAAC1",
        position: 0,
        wipLimit: 3,
        items: primary
          ? [
              {
                appId: primary.appId,
                title: primary.title,
                progress: primary.progress,
                position: 0,
                queuePosition: 0,
                plannedFor: "2026-08-14",
                objective: "Llegar al observatorio",
                targetDate: "2026-08-31",
                estimatedMinutes: 540,
              },
            ]
          : [],
      },
      {
        id: "later",
        name: "Más adelante",
        color: "#8493A4",
        position: 1,
        items: [],
      },
    ],
    queue: [],
    settings: { weeklyCapacityMinutes: 600, monthlyCapacityMinutes: 2_400 },
  };
}

/**
 * Plan denso para la vitrina: tres columnas con carga real, cola y capacidad,
 * de modo que la revisión visual juzgue la pantalla trabajando y no su estado
 * vacío.
 */
function showcasePlannerFor(items: GameSummary[]): PlannerOverview {
  const objectives = [
    "Llegar al Árbol Áureo",
    "Cerrar el acto segundo",
    "Terminar la ruta de Panam",
    "Subir al piso 40 de la Torre",
    "Completar la cosecha de otoño",
    "Desbloquear el aguijón del alma",
    "Montar la línea de placas de circuito",
    "Ganar una partida en ascensión 5",
    "Recuperar la partida abandonada",
  ];
  const plan = (index: number, position: number, plannedFor?: string) => {
    const game = items[index];
    if (!game) return undefined;
    return {
      appId: game.appId,
      title: game.title,
      coverUrl: game.coverUrl,
      progress: game.progress,
      position,
      queuePosition: position,
      plannedFor,
      objective: objectives[index % objectives.length],
      targetDate: `2026-09-${String(4 + position * 3).padStart(2, "0")}`,
      estimatedMinutes: 180 + position * 90,
    };
  };
  const compact = (values: (ReturnType<typeof plan> | undefined)[]) =>
    values.filter((value): value is NonNullable<typeof value> => Boolean(value));
  return {
    columns: [
      {
        id: "this-week",
        name: "Esta semana",
        color: "#5CAAC1",
        position: 0,
        wipLimit: 3,
        items: compact([
          plan(0, 0, "2026-08-18"),
          plan(15, 1, "2026-08-19"),
          plan(4, 2, "2026-08-21"),
        ]),
      },
      {
        id: "next-week",
        name: "Próxima semana",
        color: "#A4D007",
        position: 1,
        wipLimit: 4,
        items: compact([plan(1, 3, "2026-08-25"), plan(27, 4, "2026-08-27"), plan(42, 5)]),
      },
      {
        id: "later",
        name: "Más adelante",
        color: "#8493A4",
        position: 2,
        items: compact([plan(8, 6), plan(37, 7), plan(39, 8), plan(23, 9)]),
      },
    ],
    queue: compact([
      plan(0, 0, "2026-08-18"),
      plan(15, 1, "2026-08-19"),
      plan(4, 2, "2026-08-21"),
      plan(1, 3, "2026-08-25"),
      plan(27, 4, "2026-08-27"),
    ]),
    settings: { weeklyCapacityMinutes: 900, monthlyCapacityMinutes: 3_600 },
  };
}

const showcaseCollections: CollectionSummary[] = [
  {
    id: "favorites",
    name: "Favoritos",
    description: "Los mundos a los que siempre merece la pena volver.",
    color: "#D6A64B",
    icon: "star",
    kind: "manual",
    matchMode: "all",
    position: 0,
    gameCount: 10,
  },
  {
    id: "short-sessions",
    name: "Sesiones cortas",
    description: "Menos de una hora por partida, sin hilo argumental que perder.",
    color: "#5CAAC1",
    icon: "bolt",
    kind: "smart",
    matchMode: "all",
    position: 1,
    gameCount: 12,
  },
  {
    id: "coop-friday",
    name: "Viernes en cooperativo",
    description: "Para jugar con tres personas más sin explicar nada durante media hora.",
    color: "#A4D007",
    icon: "users",
    kind: "manual",
    matchMode: "all",
    position: 2,
    gameCount: 6,
  },
  {
    id: "unfinished-stories",
    name: "Historias a medias",
    description: "Campañas entre el 20 % y el 80 %: lo que de verdad conviene retomar.",
    color: "#5CAAC1",
    icon: "bookmark",
    kind: "smart",
    matchMode: "all",
    position: 3,
    gameCount: 14,
  },
  {
    id: "drm-free",
    name: "Sin DRM",
    description: "Títulos que se pueden conservar y ejecutar sin depender de un cliente.",
    color: "#7EA64B",
    icon: "shield",
    kind: "smart",
    matchMode: "any",
    position: 4,
    gameCount: 9,
  },
  {
    id: "winter-backlog",
    name: "Pendientes de invierno",
    description: "Compras de rebajas que aún no han recibido una sola hora.",
    color: "#82939E",
    icon: "snowflake",
    kind: "manual",
    matchMode: "all",
    position: 5,
    gameCount: 17,
  },
];

/**
 * Metadatos enriquecidos de vitrina. Reproducen la forma exacta que serializa
 * `db::rich_metadata`, para que la ficha se juzgue con contenido real y no con
 * su estado vacío.
 */
function showcaseRichMetadata(entry: ShowcaseEntry) {
  return {
    detailedDescription: {
      blocks: [
        {
          kind: "paragraph" as const,
          text: `Desarrollado por ${entry.developer} y distribuido por ${entry.publisher}. La ficha recoge lo que la tienda oficial declara, sin añadir una sola palabra propia.`,
        },
        { kind: "heading" as const, level: 3, text: "Lo que encontrarás" },
        {
          kind: "list" as const,
          ordered: false,
          items: [
            `Progresión propia dentro del género ${entry.genres[0]?.toLowerCase() ?? "de aventura"}, con un ritmo sostenido de principio a fin.`,
            "Compatibilidad con mando comprobada y controles reasignables.",
            "Guardado en la nube y logros verificables desde la propia ficha.",
          ],
        },
      ],
    },
    aboutTheGame: null,
    supportedLanguages: "Español, Inglés, Francés, Alemán, Italiano, Japonés",
    websiteUrl: "https://www.example.com",
    metacriticScore: entry.rating ? entry.rating * 9 : null,
    metacriticUrl: "https://www.metacritic.com",
    requiredAge: entry.genres.includes("Terror") ? 18 : 12,
    controllerSupport: "full",
    backgroundUrl: null,
    libraryHeroUrl: showcaseHeroUrl(entry.appId),
    libraryLogoUrl: null,
    logoPosition: null,
    drmNotice: null,
    drm: {
      state: entry.appId % 3 === 0 ? "drm_free" : "steam_drm",
      evidence: [
        {
          source: "categories",
          match:
            entry.appId % 3 === 0
              ? "La ficha oficial no declara ningún aviso de DRM ni de cuenta externa."
              : "Requiere el cliente de Steam para ejecutarse.",
        },
      ],
    },
    drmCheckedAt: "2026-08-17T08:00:00Z",
    screenshots: [],
    movies: [],
  };
}

/**
 * Biblioteca de escala: mil quinientos juegos derivados del catálogo de vitrina.
 *
 * Responde a la única pregunta que no contesta una captura con cuarenta y ocho
 * títulos: qué se rompe cuando la biblioteca es realmente grande. No lleva arte
 * remoto —serían mil quinientas descargas— sino el respaldo con el título, que
 * es justo lo que hay que ver a esa escala.
 */
const SCALE_LIBRARY_SIZE = 1_500;

function buildScaleGames(): GameDetail[] {
  const base = showcaseGames;
  return Array.from({ length: SCALE_LIBRARY_SIZE }, (_, index) => {
    const source = base[index % base.length];
    if (!source) throw new Error("El catálogo de vitrina no puede estar vacío.");
    const suffix = Math.floor(index / base.length) + 1;
    return {
      ...structuredClone(source),
      appId: 900_000 + index,
      title: suffix === 1 ? source.title : `${source.title} ${suffix}`,
      coverUrl: undefined,
      headerUrl: undefined,
      heroUrl: undefined,
      manualPosition: index,
      collectionIds: [],
    };
  });
}

const showcaseStatuses: Record<string, { name: string; color: string }> = {
  playing: { name: "Jugando", color: "#5CAAC1" },
  finished: { name: "Terminados", color: "#7EA64B" },
  backlog: { name: "Pendientes", color: "#D6A64B" },
  paused: { name: "En pausa", color: "#D6A64B" },
  unclassified: { name: "Sin clasificar", color: "#8493A4" },
};

/** Catálogo de vitrina: biblioteca densa y realista para la revisión visual. */
const showcaseGames: GameDetail[] = SHOWCASE_CATALOG.map((entry, index) => {
  const status = showcaseStatuses[entry.status] ?? showcaseStatuses.unclassified;
  return game({
    appId: entry.appId,
    title: entry.title,
    coverUrl: showcaseCoverUrl(entry.appId),
    headerUrl: showcaseHeaderUrl(entry.appId),
    heroUrl: showcaseHeroUrl(entry.appId),
    statusId: entry.status,
    statusName: status?.name ?? "Sin clasificar",
    statusColor: status?.color ?? "#8493A4",
    progress: entry.progress,
    priority: entry.priority,
    rating: entry.rating,
    pinned: entry.pinned ?? false,
    tracking: entry.status === "playing",
    installed: entry.installed,
    installPath: entry.installed
      ? `/Volumes/Games/steamapps/common/${entry.title.replace(/[^A-Za-z0-9]/g, "")}`
      : undefined,
    sizeOnDisk: entry.installed ? 8_589_934_592 + index * 1_073_741_824 : undefined,
    playtimeMinutes: entry.playtimeMinutes,
    playtimeRecentMinutes: entry.recentMinutes,
    lastPlayedAt: entry.lastPlayedAt,
    releaseDate: entry.releaseDate,
    developer: entry.developer,
    publisher: entry.publisher,
    genres: entry.genres,
    shortDescription: entry.shortDescription,
    ...showcaseRichMetadata(entry),
    manualPosition: index,
    collectionIds: index % 5 === 0 ? ["favorites"] : [],
    tags: entry.status === "playing" ? ["En curso"] : [],
  });
});

const showcaseStatusDefinitions: StatusDefinition[] = Object.entries(showcaseStatuses).map(
  ([id, value], position) => ({
    id,
    name: value.name,
    color: value.color,
    position,
    builtIn: id === "unclassified" || id === "playing" || id === "finished",
    gameCount: showcaseGames.filter((item) => item.statusId === id).length,
  }),
);

export function createTestState(scenario: VindexaScenario): TestBackendState {
  const scenarioGames =
    scenario === "empty"
      ? []
      : scenario === "scale"
        ? buildScaleGames()
        : structuredClone(scenario === "showcase" ? showcaseGames : games);
  const scenarioCollections =
    scenario === "empty"
      ? []
      : structuredClone(
          scenario === "showcase" || scenario === "scale" ? showcaseCollections : collections,
        );
  const bootstrap: AppBootstrap = {
    appVersion: "0.1.0",
    stats: {
      totalGames: scenarioGames.length,
      installedGames: scenarioGames.filter((item) => item.installed).length,
      playingGames: scenarioGames.filter((item) => item.statusId === "playing").length,
      backlogGames: scenarioGames.filter((item) => item.progress < 100).length,
      trackedGames: scenarioGames.filter((item) => item.tracking).length,
      totalPlaytimeMinutes: scenarioGames.reduce((sum, item) => sum + item.playtimeMinutes, 0),
    },
    statuses: structuredClone(
      scenario === "showcase" || scenario === "scale" ? showcaseStatusDefinitions : statuses,
    ),
    collections: scenarioCollections,
    // `AppBootstrap.planner` es la lista de columnas, no la vista completa.
    planner:
      scenario === "showcase"
        ? showcasePlannerFor(scenarioGames).columns
        : plannerFor(scenarioGames).columns,
    steam:
      scenario === "empty"
        ? {
            apiKeyConfigured: false,
            apiKeyVerificationRequired: false,
            localSteamDetected: false,
            localManifestCount: 0,
          }
        : {
            account: {
              steamId: "76561198000000000",
              personaName: "Vindexa E2E",
              lastSyncAt: "2026-08-14T10:00:00Z",
              lastSyncStatus: "success",
            },
            apiKeyConfigured: true,
            apiKeyVerificationRequired: false,
            localSteamDetected: true,
            localManifestCount: 3,
          },
    preferences: {
      density: "compact",
      periodicSyncMinutes: 0,
      confirmUninstall: true,
      librarySort: "manual",
      artCacheMib: 512,
      shortcuts: {
        library: "Mod+1",
        planner: "Mod+2",
        collections: "Mod+3",
        tracking: "Mod+4",
        search: "Mod+F",
        sync: "Mod+R",
        closePanel: "Escape",
      },
    },
    databasePath: "e2e://isolated/vindexa.sqlite3",
  };
  return {
    scenario,
    bootstrap,
    games: scenarioGames,
    collections:
      scenario === "empty"
        ? {}
        : scenario === "showcase"
          ? {
              favorites: showcaseGames
                .filter((_, index) => index % 5 === 0)
                .map((item) => item.appId),
              "short-sessions": showcaseGames
                .filter((_, index) => index % 3 === 0)
                .map((item) => item.appId)
                .slice(0, 12),
              "coop-friday": showcaseGames
                .filter((_, index) => index % 7 === 0)
                .map((item) => item.appId)
                .slice(0, 6),
              "unfinished-stories": showcaseGames
                .filter((_, index) => index % 2 === 0)
                .map((item) => item.appId)
                .slice(0, 14),
              "drm-free": showcaseGames
                .filter((_, index) => index % 4 === 0)
                .map((item) => item.appId)
                .slice(0, 9),
            }
          : { favorites: [101] },
    planner:
      scenario === "showcase" ? showcasePlannerFor(scenarioGames) : plannerFor(scenarioGames),
    // React Query reintenta una vez; dos fallos hacen visible la recuperación manual.
    bootstrapFailuresRemaining: scenario === "startup-recovery" ? 2 : 0,
    commandLog: [],
  };
}
