import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameBrowser } from "@/features/library/GameBrowser";
import type { GameSummary } from "@/lib/types";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div aria-hidden="true">{title}</div>,
  prefetchArtwork: () => undefined,
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    installGame: vi.fn(),
    launchGame: vi.fn(),
    openStore: vi.fn(),
    revealInstallation: vi.fn(),
  },
  getErrorMessage: () => "No se pudo completar la acción.",
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        key: index,
        index,
        start: index * 40,
        size: 40,
        end: index * 40 + 40,
        lane: 0,
      })),
    getTotalSize: () => count * 40,
    measure: vi.fn(),
    scrollToIndex: vi.fn(),
    scrollOffset: 0,
  }),
}));

function game(overrides: Partial<GameSummary>): GameSummary {
  return {
    appId: 1,
    title: "Un juego",
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    installed: false,
    statusId: "unclassified",
    statusName: "Sin clasificar",
    statusColor: "#8493A4",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 1,
    collectionIds: [],
    drmState: "unknown",
    ...overrides,
  } as GameSummary;
}

function renderList(games: GameSummary[]) {
  return render(
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
}

/**
 * Las señas que acompañan al título en la lista.
 *
 * Se comprueban aquí, montando el navegador de verdad, porque lo que importa no
 * es que la función que las calcula devuelva lo correcto: es que lleguen a la
 * pantalla. Un cambio en cómo se pinta la fila puede dejarlas fuera sin que
 * falle nada.
 */
describe("señas de un juego en la lista", () => {
  it("un juego sin DRM lo dice, y sobre el título, nunca sobre la carátula", () => {
    renderList([game({ appId: 10, title: "Sin ataduras", drmState: "drm_free" })]);
    expect(screen.getByText(/Sin DRM/)).toBeVisible();
  });

  it("lo que no está comprobado no se marca", () => {
    // `unknown` significa «aún no se ha mirado», no «lleva DRM»: escribir algo
    // ahí sería inventarse un dato.
    renderList([game({ appId: 11, title: "Sin comprobar", drmState: "unknown" })]);
    expect(screen.queryByText(/Sin DRM/)).toBeNull();
    expect(screen.queryByText(/DRM/)).toBeNull();
  });

  it("un juego con DRM de terceros tampoco se marca como libre", () => {
    renderList([game({ appId: 12, title: "Con Denuvo", drmState: "third_party_drm" })]);
    expect(screen.queryByText(/Sin DRM/)).toBeNull();
  });

  it("las señas conviven: instalado, early access, tienda y DRM", () => {
    renderList([
      game({
        appId: 13,
        title: "Todo a la vez",
        installed: true,
        isEarlyAccess: true,
        externalStore: "epic",
        drmState: "drm_free",
      }),
    ]);
    const señas = screen.getByText(/Instalado/);
    expect(señas).toHaveTextContent("Instalado");
    expect(señas).toHaveTextContent("Early Access");
    expect(señas).toHaveTextContent("Epic Games");
    expect(señas).toHaveTextContent("Sin DRM");
  });
});
