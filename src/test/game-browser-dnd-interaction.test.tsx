import { DndContext, type DragStartEvent } from "@dnd-kit/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameBrowser } from "@/features/library/GameBrowser";
import type { GameSummary, LibraryView } from "@/lib/types";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div aria-hidden="true">{title}</div>,
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
        start: index * 58,
        size: 58,
        end: (index + 1) * 58,
        lane: 0,
      })),
    getTotalSize: () => count * 58,
    measure: vi.fn(),
  }),
}));

const game: GameSummary = {
  appId: 730,
  title: "Counter-Strike 2",
  playtimeMinutes: 120,
  playtimeRecentMinutes: 30,
  isEarlyAccess: false,
  installed: true,
  statusId: "playing",
  statusName: "Jugando",
  statusColor: "#66c0f4",
  progress: 25,
  priority: 0,
  pinned: false,
  tracking: false,
  manualPosition: 0,
};

function renderBrowser(
  view: LibraryView = "grid",
  onDragStart = vi.fn<(event: DragStartEvent) => void>(),
) {
  const onOpen = vi.fn();
  const onSelect = vi.fn();
  render(
    <DndContext onDragStart={onDragStart}>
      <TooltipProvider>
        <GameBrowser
          games={[game]}
          total={1}
          view={view}
          selected={new Set()}
          hasMore={false}
          loadingMore={false}
          onLoadMore={vi.fn()}
          onSelect={onSelect}
          onOpen={onOpen}
          manualPositioning
        />
      </TooltipProvider>
    </DndContext>,
  );
  return { onDragStart, onOpen, onSelect };
}

describe("interacción de ficha y activador de arrastre", () => {
  beforeEach(() => vi.clearAllMocks());

  it.each<LibraryView>(["grid", "list"])(
    "abre una vez con clic, Intro o Espacio en vista %s sin iniciar un arrastre",
    async (view) => {
      const user = userEvent.setup();
      const { onDragStart, onOpen, onSelect } = renderBrowser(view);
      const target = screen.getByRole("button", {
        name: "Counter-Strike 2, Jugando, 25%",
      });

      await user.click(target);
      expect(onOpen).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledTimes(1);
      expect(onDragStart).not.toHaveBeenCalled();

      onOpen.mockClear();
      onSelect.mockClear();
      target.focus();
      await user.keyboard("[Enter]");
      expect(onOpen).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledTimes(1);
      expect(onDragStart).not.toHaveBeenCalled();

      onOpen.mockClear();
      onSelect.mockClear();
      await user.keyboard("[Space]");
      expect(onOpen).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledTimes(1);
      expect(onDragStart).not.toHaveBeenCalled();
    },
  );

  it("reserva el teclado del DnD al activador dedicado", async () => {
    const user = userEvent.setup();
    const { onDragStart, onOpen } = renderBrowser();
    const handle = screen.getByRole("button", { name: "Arrastrar Counter-Strike 2" });
    handle.focus();

    await user.keyboard("[Space]");

    expect(onDragStart).toHaveBeenCalledTimes(1);
    expect(String(onDragStart.mock.calls[0]?.[0].active.id)).toBe("game:730");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("monta un ancla global estable cuando el orden manual está activo", () => {
    renderBrowser();

    expect(document.querySelector('[data-position-drop="true"]')).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Arrastrar Counter-Strike 2" })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Arrastrar Counter-Strike 2" })).toHaveAttribute(
      "aria-describedby",
    );
  });
});
