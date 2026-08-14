import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

const trackedGame: GameSummary = {
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
  forgotten: [{ ...trackedGame, appId: 20, title: "Olvidado", tracking: false, progress: 10 }],
  almostFinished: [{ ...trackedGame, appId: 30, title: "Casi listo", progress: 92 }],
  upcoming: [],
  events: [],
  officialPublications: [
    {
      gid: "1840944183772671",
      appId: 10,
      gameTitle: "Viajero",
      title: "Gameplay patch",
      contentPreview: "Notas verificadas del feed de Steam.",
      publishedAt: "2026-07-30T23:58:15+00:00",
      feedLabel: "Community Announcements",
      feedName: "steam_community_announcements",
    },
  ],
  relatedReleases: [
    {
      appId: 40,
      title: "Viajero II",
      releaseDate: "2026-12-10",
      relatedToAppId: 10,
      relatedToTitle: "Viajero",
      criterion: "developer",
      criterionValue: "forge one",
    },
  ],
  dismissedRecommendations: [],
  capabilities: {
    metadataObservations: 0,
    earlyAccessHistoryAvailable: false,
    trackedNewsGames: 1,
    officialPublicationsAvailable: true,
    newsLastRefreshedAt: "2026-08-14T18:00:00+00:00",
    newsNextRefreshAt: "2026-08-15T00:00:00+00:00",
    relatedReleasesAvailable: true,
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

describe("seguimiento y descubrimiento", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.discoverySnapshot.mockResolvedValue(snapshot);
    mockedApi.refreshDiscoveryNews.mockResolvedValue({
      attemptedGames: 1,
      refreshedGames: 1,
      publicationsSaved: 1,
      failedGames: 0,
      skippedByCadence: 0,
      nextRefreshAt: "2026-08-15T00:00:00+00:00",
    });
    mockedApi.listGames.mockResolvedValue({
      items: [trackedGame],
      total: 1,
      limit: 200,
      offset: 0,
    });
    mockedApi.recommendGame.mockResolvedValue({
      historyId: "123e4567-e89b-12d3-a456-426614174000",
      game: trackedGame,
      reasons: ["Está en seguimiento", "Tu estimación encaja en 60 minutos"],
    });
    mockedApi.saveReminder.mockResolvedValue({
      id: "123e4567-e89b-12d3-a456-426614174001",
      appId: 20,
      title: "Olvidado",
      dueAt: "2026-08-21T10:00:00Z",
      note: "Retomar Olvidado",
    });
    mockedApi.dismissRecommendation.mockResolvedValue(undefined);
  });

  it("expone publicaciones y relaciones con su procedencia verificable", async () => {
    renderScreen();

    expect(await screen.findByRole("heading", { name: "Qué jugar ahora" })).toBeVisible();
    expect(screen.getByRole("tab", { name: /Seguimiento/ })).toBeVisible();
    expect(screen.getByRole("tab", { name: /Recordatorios/ })).toBeVisible();
    expect(screen.getByRole("tab", { name: /Olvidados/ })).toBeVisible();
    expect(screen.getByRole("tab", { name: /Casi terminados/ })).toBeVisible();
    expect(await screen.findByText("Cambios de Early Access")).toBeVisible();
    expect(screen.getByText("Publicaciones oficiales recientes")).toBeVisible();
    expect(screen.getByText("Gameplay patch")).toBeVisible();
    expect(screen.getByText(/Este método no expone una señal de importancia/i)).toBeVisible();
    expect(screen.getByText("Lanzamientos relacionados")).toBeVisible();
    expect(screen.getByText("Viajero II")).toBeVisible();
    expect(screen.getByText(/Mismo desarrollador · forge one/i)).toBeVisible();
    expect(screen.getByRole("heading", { name: "Recomendaciones descartadas" })).toBeVisible();
  });

  it("crea un recordatorio persistente desde un juego olvidado", async () => {
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("tab", { name: /Olvidados/ }));
    expect(screen.getByText("Olvidado")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Recordarme" }));

    await waitFor(() => expect(mockedApi.saveReminder).toHaveBeenCalledTimes(1));
    expect(mockedApi.saveReminder.mock.calls[0]?.[0]).toMatchObject({
      appId: 20,
      note: "Retomar Olvidado",
    });
  });

  it("explica y permite descartar una recomendación", async () => {
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("button", { name: "Elige por mí" }));
    expect(await screen.findByRole("heading", { name: "Viajero" })).toBeVisible();
    expect(screen.getByText("Está en seguimiento")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "No me apetece" }));
    await waitFor(() =>
      expect(mockedApi.dismissRecommendation).toHaveBeenCalledWith(
        "123e4567-e89b-12d3-a456-426614174000",
      ),
    );
  });

  it("mantiene un error recuperable cuando el feed oficial no responde", async () => {
    const user = userEvent.setup();
    mockedApi.discoverySnapshot.mockResolvedValue({
      ...snapshot,
      officialPublications: [],
      capabilities: {
        ...snapshot.capabilities,
        officialPublicationsAvailable: false,
      },
    });
    mockedApi.refreshDiscoveryNews.mockRejectedValue(new Error("Steam no respondió a tiempo"));
    renderScreen();

    expect(await screen.findByText("Steam no respondió a tiempo")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Reintentar" }));
    await waitFor(() => expect(mockedApi.refreshDiscoveryNews).toHaveBeenCalledTimes(2));
  });

  it("distingue el estado vacío sin seguimiento del estado de carga", async () => {
    mockedApi.discoverySnapshot.mockResolvedValueOnce({
      ...snapshot,
      officialPublications: [],
      capabilities: {
        ...snapshot.capabilities,
        trackedNewsGames: 0,
        officialPublicationsAvailable: false,
      },
    });
    const first = renderScreen();

    expect(
      await screen.findByText(
        "Sigue al menos un juego para consultar su feed sin usar tu Web API Key.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Actualizar" })).toBeDisabled();
    first.unmount();

    mockedApi.discoverySnapshot.mockResolvedValueOnce({
      ...snapshot,
      officialPublications: [],
      capabilities: {
        ...snapshot.capabilities,
        officialPublicationsAvailable: false,
      },
    });
    mockedApi.refreshDiscoveryNews.mockImplementationOnce(() => new Promise(() => {}));
    renderScreen();

    const loading = await screen.findByText("Contrastando el feed oficial…");
    expect(loading).toHaveAttribute("role", "status");
  });
});
