import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameBrowser } from "@/features/library/GameBrowser";
import { api } from "@/lib/tauri";
import type { GameSummary } from "@/lib/types";

const VISIBLE_WINDOW = 14;
const cacheGameArt = vi.hoisted(() => vi.fn());
const launchGame = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  api: {
    cacheGameArt,
    launchGame,
    installGame: vi.fn(),
    openStore: vi.fn(),
    revealInstallation: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la acción.",
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: Math.min(count, VISIBLE_WINDOW) }, (_, index) => ({
        key: index,
        index,
        start: index * 58,
        size: 58,
        end: (index + 1) * 58,
        lane: 0,
      })),
    getTotalSize: () => count * 58,
    measure: vi.fn(),
    scrollToIndex: vi.fn(),
  }),
}));

const games: GameSummary[] = Array.from({ length: 5_000 }, (_, index) => ({
  appId: index + 1,
  title: `Juego ${String(index + 1).padStart(4, "0")}`,
  coverUrl: `https://shared.steamstatic.com/store_item_assets/steam/apps/${index + 1}/cover.jpg`,
  playtimeMinutes: index * 10,
  playtimeRecentMinutes: index % 120,
  isEarlyAccess: false,
  installed: index % 4 === 0,
  statusId: "unclassified",
  statusName: "Sin clasificar",
  statusColor: "#6F7B8A",
  progress: index % 101,
  priority: 0,
  pinned: false,
  tracking: false,
  manualPosition: index,
}));

describe("ventana virtual para bibliotecas grandes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    cacheGameArt.mockImplementation(async (appId: number) => ({
      appId,
      variant: "cover",
      localPath: `/cache/${appId}/cover.jpg`,
    }));
  });

  it("recibe 5000 juegos pero monta y solicita artwork solo para la ventana visible", async () => {
    const { container } = render(
      <TooltipProvider>
        <GameBrowser
          games={games}
          total={games.length}
          view="list"
          selected={new Set()}
          hasMore={false}
          loadingMore={false}
          onLoadMore={vi.fn()}
          onSelect={vi.fn()}
          onOpen={vi.fn()}
        />
      </TooltipProvider>,
    );

    const mountedRows = container.querySelectorAll(".game-row");
    expect(mountedRows).toHaveLength(VISIBLE_WINDOW);
    expect(container).toHaveTextContent("Juego 0001");
    expect(container).not.toHaveTextContent("Juego 5000");
    expect(container.querySelector(".virtual-canvas")).toHaveStyle({
      height: `${games.length * 58}px`,
    });
    await waitFor(() => expect(cacheGameArt).toHaveBeenCalledTimes(VISIBLE_WINDOW));
  });

  it("virtualiza también la cuadrícula y no crea 5000 descargas", async () => {
    const { container } = render(
      <TooltipProvider>
        <GameBrowser
          games={games}
          total={games.length}
          view="grid"
          selected={new Set()}
          hasMore={false}
          loadingMore={false}
          onLoadMore={vi.fn()}
          onSelect={vi.fn()}
          onOpen={vi.fn()}
        />
      </TooltipProvider>,
    );

    const mountedCards = container.querySelectorAll(".game-card");
    expect(mountedCards.length).toBeGreaterThan(0);
    expect(mountedCards.length).toBeLessThan(100);
    expect(container).toHaveTextContent("Juego 0001");
    expect(container).not.toHaveTextContent("Juego 5000");
    await waitFor(() => expect(cacheGameArt).toHaveBeenCalledTimes(mountedCards.length));
  });

  it("mantiene la vista ultracompacta virtualizada y semánticamente completa", async () => {
    const { container } = render(
      <TooltipProvider>
        <GameBrowser
          games={games}
          total={games.length}
          view="compact"
          selected={new Set()}
          hasMore={false}
          loadingMore={false}
          onLoadMore={vi.fn()}
          onSelect={vi.fn()}
          onOpen={vi.fn()}
        />
      </TooltipProvider>,
    );

    expect(container.querySelector(".game-browser--compact")).toBeInTheDocument();
    expect(container.querySelectorAll(".game-row")).toHaveLength(VISIBLE_WINDOW);
    // La ultracompacta cambia la carátula y la barra dibujada por texto: es lo
    // que le permite bajar de 38 a 26 px por fila.
    expect(container.querySelectorAll("img")).toHaveLength(0);
    expect(container.querySelectorAll("[data-slot='progress']")).toHaveLength(0);
    expect(container.querySelectorAll(".game-row__progress span")[0]).toHaveTextContent("0%");
    expect(screen.getByRole("button", { name: /Juego 0001, Sin clasificar/ })).toBeVisible();
  });

  it("expone la selección múltiple en el botón interactivo", () => {
    render(
      <TooltipProvider>
        <GameBrowser
          games={games.slice(0, 2)}
          total={2}
          view="grid"
          selected={new Set([1])}
          hasMore={false}
          loadingMore={false}
          onLoadMore={vi.fn()}
          onSelect={vi.fn()}
          onOpen={vi.fn()}
        />
      </TooltipProvider>,
    );

    expect(screen.getByRole("button", { name: /Juego 0001, Sin clasificar/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: /Juego 0002, Sin clasificar/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("muestra el error exacto de una acción de Steam en una alerta accesible", async () => {
    const user = userEvent.setup();
    vi.mocked(api.launchGame).mockRejectedValueOnce(new Error("Steam no está disponible"));
    render(
      <TooltipProvider>
        <GameBrowser
          games={games.slice(0, 1)}
          total={1}
          view="list"
          selected={new Set()}
          hasMore={false}
          loadingMore={false}
          onLoadMore={vi.fn()}
          onSelect={vi.fn()}
          onOpen={vi.fn()}
        />
      </TooltipProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Acciones para Juego 0001" }));
    await user.click(screen.getByRole("menuitem", { name: "Jugar" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Juego 0001: Steam no está disponible",
    );
  });
});
