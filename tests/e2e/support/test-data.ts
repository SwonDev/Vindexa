import type {
  AppBootstrap,
  CollectionSummary,
  GameDetail,
  GameSummary,
  PlannerOverview,
  StatusDefinition,
} from "../../../src/lib/types";

export type VindexaScenario =
  | "empty"
  | "library"
  | "library-error"
  | "database-recovery"
  | "startup-error"
  | "startup-recovery";

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

export function createTestState(scenario: VindexaScenario): TestBackendState {
  const scenarioGames = scenario === "empty" ? [] : structuredClone(games);
  const scenarioCollections = scenario === "empty" ? [] : structuredClone(collections);
  const bootstrap: AppBootstrap = {
    stats: {
      totalGames: scenarioGames.length,
      installedGames: scenarioGames.filter((item) => item.installed).length,
      playingGames: scenarioGames.filter((item) => item.statusId === "playing").length,
      backlogGames: scenarioGames.filter((item) => item.progress < 100).length,
      trackedGames: scenarioGames.filter((item) => item.tracking).length,
      totalPlaytimeMinutes: scenarioGames.reduce((sum, item) => sum + item.playtimeMinutes, 0),
    },
    statuses: structuredClone(statuses),
    collections: scenarioCollections,
    planner: plannerFor(scenarioGames),
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
    collections: scenario === "empty" ? {} : { favorites: [101] },
    planner: plannerFor(scenarioGames),
    // React Query reintenta una vez; dos fallos hacen visible la recuperación manual.
    bootstrapFailuresRemaining: scenario === "startup-recovery" ? 2 : 0,
    commandLog: [],
  };
}
