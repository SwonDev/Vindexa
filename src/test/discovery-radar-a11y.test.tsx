import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DiscoveryScreen } from "@/features/discovery/DiscoveryScreen";
import { api } from "@/lib/tauri";
import type { AppBootstrap, DiscoverySnapshot, GameSummary } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    discoverySnapshot: vi.fn(),
    refreshDiscoveryNews: vi.fn(),
    listGames: vi.fn(),
    recommendGame: vi.fn(),
    saveReminder: vi.fn(),
    completeReminder: vi.fn(),
    snoozeReminder: vi.fn(),
    dismissRecommendation: vi.fn(),
    restoreRecommendation: vi.fn(),
    launchGame: vi.fn(),
    openStore: vi.fn(),
    cacheGameArt: vi.fn(),
    upcomingReleases: vi.fn(),
    dismissUpcomingRelease: vi.fn(),
    scoreUpcomingReleases: vi.fn(),
    recordTasteFeedback: vi.fn(),
    learnTaste: vi.fn(),
    listNotificationRules: vi.fn(),
    saveNotificationRule: vi.fn(),
    deleteNotificationRule: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

const game: GameSummary = {
  appId: 10,
  title: "Viajero",
  playtimeMinutes: 180,
  playtimeRecentMinutes: 20,
  lastPlayedAt: "2025-01-01T00:00:00Z",
  isEarlyAccess: false,
  installed: true,
  statusId: "playing",
  statusName: "Jugando",
  statusColor: "#5CAAC1",
  progress: 80,
  priority: 3,
  pinned: false,
  tracking: true,
  manualPosition: 0,
};

const snapshot: DiscoverySnapshot = {
  reminders: [],
  forgotten: [{ ...game, appId: 20, title: "Olvidado", tracking: false, progress: 10 }],
  almostFinished: [{ ...game, appId: 30, title: "Casi listo", progress: 92 }],
  upcoming: [],
  events: [],
  officialPublications: [],
  relatedReleases: [],
  dismissedRecommendations: [],
  capabilities: {
    metadataObservations: 4,
    earlyAccessHistoryAvailable: false,
    trackedNewsGames: 1,
    officialPublicationsAvailable: true,
    relatedReleasesAvailable: false,
  },
};

const bootstrap: AppBootstrap = {
  stats: {
    totalGames: 3,
    installedGames: 1,
    playingGames: 1,
    backlogGames: 1,
    trackedGames: 1,
    totalPlaytimeMinutes: 180,
  },
  statuses: [],
  collections: [],
  planner: [],
  steam: {
    apiKeyConfigured: true,
    apiKeyVerificationRequired: false,
    localSteamDetected: true,
    localManifestCount: 1,
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
  databasePath: "/tmp/vindexa.sqlite3",
};

function renderScreen() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DiscoveryScreen bootstrap={bootstrap} loading={false} />
    </QueryClientProvider>,
  );
}

describe("accesibilidad del radar personal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.discoverySnapshot.mockResolvedValue(snapshot);
    mockedApi.refreshDiscoveryNews.mockResolvedValue({
      attemptedGames: 1,
      refreshedGames: 1,
      publicationsSaved: 0,
      failedGames: 0,
      skippedByCadence: 0,
    });
    mockedApi.listGames.mockResolvedValue({ items: [game], total: 1, limit: 200, offset: 0 });
    mockedApi.upcomingReleases.mockResolvedValue([]);
    mockedApi.listNotificationRules.mockResolvedValue([]);
  });

  it("expone un tablist horizontal conectado con su panel", async () => {
    // Las cuatro vistas viven en fila sobre la lista que filtran desde que se
    // quitó la columna de la izquierda, que ocupaba un cuarto de la pantalla
    // para seis controles. Lo que se anuncia tiene que coincidir con lo que se
    // ve: un lector de pantalla que oye «vertical» y encuentra una fila deja de
    // saber qué flecha usar.
    renderScreen();

    const tablist = await screen.findByRole("tablist", { name: "Radar personal" });
    expect(tablist).toHaveAttribute("aria-orientation", "horizontal");

    // Sólo las del radar: la columna de señales tiene su propio tablist, y
    // mezclarlos haría que las flechas de uno movieran el otro.
    const tabs = within(tablist).getAllByRole("tab");
    expect(tabs).toHaveLength(4);
    for (const tab of tabs) {
      expect(tab).toHaveAttribute("aria-controls", "radar-panel");
    }

    const panel = document.querySelector("#radar-panel");
    expect(panel).not.toBeNull();
    expect(panel).toHaveAttribute("role", "tabpanel");
    expect(panel).toHaveAttribute("aria-labelledby", "radar-tab-tracking");
  });

  it("mantiene un único punto de tabulación y navega con las flechas", async () => {
    const user = userEvent.setup();
    renderScreen();

    const tracking = await screen.findByRole("tab", { name: /Seguimiento/ });
    const reminders = screen.getByRole("tab", { name: /Recordatorios/ });
    expect(tracking).toHaveAttribute("tabindex", "0");
    expect(reminders).toHaveAttribute("tabindex", "-1");

    tracking.focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("tab", { name: /Recordatorios/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: /Recordatorios/ })).toHaveFocus();

    await user.keyboard("{End}");
    expect(screen.getByRole("tab", { name: /Casi terminados/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );

    await user.keyboard("{Home}");
    expect(screen.getByRole("tab", { name: /Seguimiento/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("anuncia el recuento de la vista activa en una región viva", async () => {
    const user = userEvent.setup();
    renderScreen();

    const status = await screen.findByText("1 elementos en esta vista");
    expect(status).toHaveAttribute("aria-live", "polite");

    await user.click(screen.getByRole("tab", { name: /Olvidados/ }));
    expect(document.querySelector("#radar-panel")).toHaveAttribute(
      "aria-labelledby",
      "radar-tab-forgotten",
    );
    expect(screen.getByRole("heading", { level: 2, name: "Olvidados" })).toBeVisible();
  });

  /**
   * Cada bloque de señales vive en un grupo, y en uno solo.
   *
   * Eran nueve bloques apilados en una columna con desplazamiento: lo que
   * caduca hoy quedaba a la misma altura visual que el histórico de
   * descartados. Al repartirlos en tres grupos, el riesgo nuevo es el
   * contrario: que uno se quede fuera de todos y desaparezca sin que falle
   * nada. Esta prueba los enumera y los busca donde deben estar.
   */
  it("reparte los bloques de señales en tres grupos, sin perder ninguno", async () => {
    const user = userEvent.setup();
    renderScreen();

    expect(
      await screen.findByRole("complementary", { name: /Señales, novedades y avisos/ }),
    ).toBeVisible();
    const grupos = await screen.findByRole("tablist", { name: "Señales" });
    expect(grupos).toHaveAttribute("aria-orientation", "horizontal");
    expect(within(grupos).getAllByRole("tab")).toHaveLength(3);

    const reparto: [string, string[]][] = [
      // Lo que caduca. Es el grupo con el que abre la columna.
      ["Oportunidades", ["Ofertas para ti", "Próximos lanzamientos para ti"]],
      [
        "Novedades",
        [
          "Publicaciones oficiales recientes",
          "Lanzamientos relacionados",
          "Próximos de tu biblioteca",
          "Cambios de Early Access",
        ],
      ],
      ["Avisos", ["Avisos programados", "Recomendaciones descartadas"]],
    ];

    for (const [grupo, titulos] of reparto) {
      await user.click(within(grupos).getByRole("tab", { name: grupo }));
      for (const title of titulos) {
        expect(
          await screen.findByRole("heading", { level: 2, name: title }),
          `«${title}» tiene que estar en «${grupo}»`,
        ).toBeVisible();
      }
    }
  });
});
