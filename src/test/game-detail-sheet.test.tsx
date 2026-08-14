import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameDetailSheet } from "@/features/library/GameDetailSheet";
import { api } from "@/lib/tauri";
import type { GameDetail } from "@/lib/types";
import "@/index.css";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title, kind }: { title: string; kind: string }) => (
    <div className="artwork" data-kind={kind} role="img" aria-label={`Arte de ${title}`} />
  ),
}));
vi.mock("@/lib/tauri", () => ({
  api: {
    gameDetail: vi.fn(),
    refreshGameMetadata: vi.fn(),
    refreshGameAchievements: vi.fn(),
    updateGame: vi.fn(),
    setGameCollections: vi.fn(),
    listTags: vi.fn(),
    saveTag: vi.fn(),
    deleteTag: vi.fn(),
    setGameTags: vi.fn(),
    listGameSessions: vi.fn(),
    saveGameSession: vi.fn(),
    deleteGameSession: vi.fn(),
    savePersonalDates: vi.fn(),
    launchGame: vi.fn(),
    installGame: vi.fn(),
    uninstallGame: vi.fn(),
    revealInstallation: vi.fn(),
    openStore: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "Error inesperado",
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;
const detail = {
  appId: 620,
  title: "Portal 2",
  coverUrl: "https://example.test/cover.jpg",
  headerUrl: "https://example.test/header.jpg",
  heroUrl: "https://example.test/hero.jpg",
  playtimeMinutes: 800,
  playtimeRecentMinutes: 20,
  lastPlayedAt: "2026-08-14T10:00:00Z",
  releaseDate: "2011-04-19",
  isEarlyAccess: false,
  steamDeckStatus: "Verificado",
  achievementsUnlocked: undefined,
  achievementsTotal: 51,
  installed: true,
  installPath: "/Games/Portal 2",
  sizeOnDisk: 12_000_000_000,
  statusId: "playing",
  statusName: "Jugando",
  statusColor: "#5CAAC1",
  progress: 55,
  priority: 4,
  pinned: true,
  tracking: true,
  rating: 10,
  manualPosition: 0,
  shortDescription: "Una aventura cooperativa entre portales.",
  developer: "Valve",
  publisher: "Valve",
  genres: ["Acción", "Aventura"],
  categories: ["Cooperativo"],
  metadataStatus: "success",
  metadataFetchedAt: "2026-08-14T10:00:00Z",
  achievementsStatus: "pending",
  achievementsFetchedAt: undefined,
  isFree: false,
  ownershipSource: "owned",
  familyAvailability: "not_applicable",
  collectionIds: [],
  tags: [],
  sessions: [],
  activity: [],
} as GameDetail;

function renderSheet(onOpenChange = vi.fn()) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <GameDetailSheet
          appId={620}
          open
          onOpenChange={onOpenChange}
          statuses={[
            {
              id: "playing",
              name: "Jugando",
              color: "#5CAAC1",
              position: 0,
              builtIn: true,
              gameCount: 1,
            },
          ]}
          collections={[]}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("ficha inmersiva de juego", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.gameDetail.mockResolvedValue(detail);
    mockedApi.refreshGameMetadata.mockResolvedValue(detail);
    mockedApi.refreshGameAchievements.mockResolvedValue({
      ...detail,
      achievementsStatus: "success",
      achievementsUnlocked: 0,
    });
    mockedApi.updateGame.mockResolvedValue(detail);
    mockedApi.listTags.mockResolvedValue([]);
    mockedApi.setGameTags.mockResolvedValue(detail);
    mockedApi.listGameSessions.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    mockedApi.saveGameSession.mockResolvedValue(detail);
    mockedApi.deleteGameSession.mockResolvedValue(detail);
    mockedApi.savePersonalDates.mockResolvedValue(detail);
    mockedApi.launchGame.mockResolvedValue(undefined);
  });

  it("muestra metadata real, hero dedicado y no inventa logros desbloqueados", async () => {
    const { container } = renderSheet();
    expect(await screen.findByRole("heading", { name: "Portal 2", level: 2 })).toBeVisible();
    expect(screen.getByText("Una aventura cooperativa entre portales.")).toBeVisible();
    expect(screen.getByText("Sin datos")).toBeVisible();
    expect(
      container.ownerDocument.querySelector(".detail-hero__media [data-kind='hero']"),
    ).toBeTruthy();
    await waitFor(() => expect(mockedApi.refreshGameMetadata).toHaveBeenCalledWith(620, false));
  });

  it("sincroniza logros sólo bajo petición y muestra 0/N únicamente con respuesta oficial", async () => {
    const user = userEvent.setup();
    renderSheet();
    expect(mockedApi.refreshGameAchievements).not.toHaveBeenCalled();
    await user.click(await screen.findByRole("button", { name: "Actualizar logros" }));
    await waitFor(() => expect(mockedApi.refreshGameAchievements).toHaveBeenCalledWith(620));
    expect(await screen.findByText("0/51")).toBeVisible();
  });

  it("expone registro accesible para etiquetas, sesiones y fechas personales", async () => {
    const user = userEvent.setup();
    renderSheet();
    await user.click(await screen.findByRole("tab", { name: "Registro" }));
    expect(screen.getByRole("heading", { name: "Fechas personales" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Etiquetas" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Sesiones de juego" })).toBeVisible();

    await user.type(screen.getByLabelText("Fecha de inicio personal"), "2026-08-10");
    await user.click(screen.getByRole("button", { name: "Guardar fechas" }));
    await waitFor(() =>
      expect(mockedApi.savePersonalDates).toHaveBeenCalledWith({
        appId: 620,
        startedAt: "2026-08-10",
        completedAt: undefined,
        abandonedAt: undefined,
      }),
    );
  });

  it("distingue un fallo de etiquetas del estado vacío y permite reintentar", async () => {
    const user = userEvent.setup();
    mockedApi.listTags.mockRejectedValueOnce(new Error("SQLite no respondió."));
    renderSheet();
    await user.click(await screen.findByRole("tab", { name: "Registro" }));

    const error = await screen.findByRole("alert");
    expect(error).toHaveTextContent("No se pudieron cargar tus etiquetas.");
    expect(screen.queryByText("Aún no has creado etiquetas personales.")).not.toBeInTheDocument();
    await user.click(within(error).getByRole("button", { name: "Reintentar" }));
    expect(await screen.findByText("Aún no has creado etiquetas personales.")).toBeVisible();
    expect(mockedApi.listTags).toHaveBeenCalledTimes(2);
  });

  it("carga sesiones antiguas por páginas sin ocultar el historial", async () => {
    const user = userEvent.setup();
    const paginatedDetail = {
      ...detail,
      sessionsTotal: 2,
      sessions: [
        {
          id: "recent",
          startedAt: "2026-08-14T18:00:00Z",
          note: "Sesión reciente",
        },
      ],
    } as GameDetail;
    mockedApi.gameDetail.mockResolvedValueOnce(paginatedDetail);
    mockedApi.refreshGameMetadata.mockResolvedValueOnce(paginatedDetail);
    mockedApi.listGameSessions.mockResolvedValueOnce({
      items: [
        {
          id: "old",
          startedAt: "2026-01-10T18:00:00Z",
          note: "Sesión antigua conservada",
        },
      ],
      total: 2,
      limit: 50,
      offset: 1,
    });
    renderSheet();
    await user.click(await screen.findByRole("tab", { name: "Registro" }));

    expect(await screen.findByText("Sesión reciente")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Cargar sesiones anteriores" }));
    expect(await screen.findByText("Sesión antigua conservada")).toBeVisible();
    expect(mockedApi.listGameSessions).toHaveBeenCalledWith(620, 50, 1);
  });

  it("expone error exacto, evita ejecución doble y permite reintentar la acción", async () => {
    const user = userEvent.setup();
    mockedApi.launchGame.mockRejectedValueOnce(new Error("Steam no está disponible."));
    renderSheet();
    const play = await screen.findByRole("button", { name: "Jugar" });
    await user.click(play);
    expect(await screen.findByRole("alert")).toHaveTextContent("Steam no está disponible.");
    expect(mockedApi.launchGame).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Reintentar" }));
    expect(await screen.findByText("Steam recibió la orden de iniciar el juego.")).toBeVisible();
    expect(mockedApi.launchGame).toHaveBeenCalledTimes(2);
  });

  it("vacía el debounce al cerrar y conserva el último formulario aunque se desmonte", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    mockedApi.updateGame.mockImplementation(async (input) => ({ ...detail, notes: input.notes }));
    const view = renderSheet(onOpenChange);
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });

    await user.type(
      screen.getByLabelText("Notas privadas"),
      "Este cambio debe sobrevivir al cierre inmediato.",
    );
    await user.click(screen.getByRole("button", { name: "Close" }));
    view.unmount();

    await waitFor(() =>
      expect(mockedApi.updateGame).toHaveBeenCalledWith(
        expect.objectContaining({ notes: "Este cambio debe sobrevivir al cierre inmediato." }),
      ),
    );
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("encola una edición nueva durante un guardado y no la pisa con la respuesta anterior", async () => {
    const user = userEvent.setup();
    let resolveFirst: (value: GameDetail) => void = () => undefined;
    mockedApi.updateGame
      .mockImplementationOnce(
        () =>
          new Promise<GameDetail>((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockImplementationOnce(async (input) => ({ ...detail, notes: input.notes }));
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });
    const notes = screen.getByLabelText("Notas privadas");

    await user.type(notes, "Primera edición");
    await waitFor(() => expect(mockedApi.updateGame).toHaveBeenCalledTimes(1));
    await user.clear(notes);
    await user.type(notes, "Edición más reciente");
    await new Promise((resolve) => window.setTimeout(resolve, 750));
    resolveFirst({ ...detail, notes: "Primera edición" });

    await waitFor(() => expect(mockedApi.updateGame).toHaveBeenCalledTimes(2));
    expect(mockedApi.updateGame).toHaveBeenLastCalledWith(
      expect.objectContaining({ notes: "Edición más reciente" }),
    );
    await waitFor(() => expect(notes).toHaveValue("Edición más reciente"));
  });

  it("confirma antes de solicitar a Steam la desinstalación y comunica el traspaso", async () => {
    const user = userEvent.setup();
    mockedApi.uninstallGame.mockResolvedValue(undefined);
    renderSheet();
    await user.click(await screen.findByRole("button", { name: "Desinstalar" }));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toHaveTextContent(/Vindexa no borrará archivos directamente/);
    expect(mockedApi.uninstallGame).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Solicitar a Steam" }));
    await waitFor(() => expect(mockedApi.uninstallGame).toHaveBeenCalledWith(620));
    expect(await screen.findByText(/Steam recibió la solicitud de desinstalación/)).toBeVisible();
  });

  it("actualiza el parallax por scroll mediante transform sin recalcular layout", async () => {
    const { container } = renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });
    const scroller = container.ownerDocument.querySelector(".game-detail-sheet") as HTMLDivElement;
    Object.defineProperty(scroller, "scrollTop", { configurable: true, value: 200 });
    fireEvent.scroll(scroller);
    await waitFor(() => {
      expect(
        container.ownerDocument.querySelector<HTMLElement>(".detail-hero__media")?.style.transform,
      ).toContain("translate3d(0, 36px, 0)");
    });
  });

  it("mantiene el hero estable cuando el sistema pide reducir el movimiento", async () => {
    const matchMedia = vi.spyOn(window, "matchMedia").mockReturnValue({
      matches: true,
      media: "(prefers-reduced-motion: reduce)",
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    });
    const { container } = renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });
    expect(
      container.ownerDocument.querySelector<HTMLElement>(".detail-hero__media")?.style.transform,
    ).toBe("none");
    matchMedia.mockRestore();
  });

  it("permite forzar un reintento explícito cuando la ficha de Steam falló", async () => {
    const user = userEvent.setup();
    const failedDetail = {
      ...detail,
      metadataStatus: "failed",
      shortDescription: undefined,
    } as GameDetail;
    mockedApi.gameDetail.mockResolvedValueOnce(failedDetail);
    mockedApi.refreshGameMetadata.mockResolvedValueOnce(failedDetail).mockResolvedValueOnce(detail);
    renderSheet();
    await user.click(await screen.findByRole("button", { name: "Reintentar ficha" }));
    await waitFor(() => expect(mockedApi.refreshGameMetadata).toHaveBeenCalledWith(620, true));
  });

  it("mantiene overview y pestañas sin compresión ni solape con una descripción extensa", async () => {
    mockedApi.gameDetail.mockResolvedValueOnce({
      ...detail,
      shortDescription: "Descripción extensa y legible. ".repeat(80),
    });
    renderSheet();
    await screen.findByRole("heading", { name: "Portal 2", level: 2 });
    const overview = document.querySelector<HTMLElement>(".detail-overview");
    const tabs = document.querySelector<HTMLElement>(".detail-tabs");
    expect(overview).toBeTruthy();
    expect(tabs).toBeTruthy();
    expect(getComputedStyle(overview as HTMLElement).flexShrink).toBe("0");
    expect(getComputedStyle(tabs as HTMLElement).flexShrink).toBe("0");
    expect(
      overview?.compareDocumentPosition(tabs as Node) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
