import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameBrowser } from "@/features/library/GameBrowser";
import type { GameSummary, LibraryView } from "@/lib/types";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div aria-hidden="true">{title}</div>,
  // La precarga es una mejora de tiempos: en pruebas basta con que exista.
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

/**
 * El virtualizador de mentira reproduce la geometría real —cada fila mide lo
 * que dice `estimateSize`— porque el encabezado fijo y el índice de salto se
 * deciden justo con esas medidas.
 */
const virtual = vi.hoisted(() => ({ scrollOffset: 0, scrollToIndex: vi.fn() }));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({
    count,
    estimateSize,
  }: {
    count: number;
    estimateSize: (index: number) => number;
  }) => {
    const items: {
      key: number;
      index: number;
      start: number;
      size: number;
      end: number;
      lane: number;
    }[] = [];
    let start = 0;
    for (let index = 0; index < count; index += 1) {
      const size = estimateSize(index);
      items.push({ key: index, index, start, size, end: start + size, lane: 0 });
      start += size;
    }
    return {
      getVirtualItems: () => items,
      getTotalSize: () => start,
      measure: vi.fn(),
      scrollToIndex: virtual.scrollToIndex,
      scrollOffset: virtual.scrollOffset,
    };
  },
}));

function game(appId: number, title: string): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isFree: false,
    drmState: "unknown",
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    genres: [],
    isEarlyAccess: false,
    installed: false,
    statusId: "unclassified",
    statusName: "Sin clasificar",
    statusColor: "#8493A4",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: appId,
    collectionIds: [],
  };
}

const games = [
  game(1, "Abzû"),
  game(2, "Alba"),
  game(3, "Braid"),
  game(4, "Celeste"),
  game(5, "Control"),
  game(6, "Dead Cells"),
];

function renderBrowser(
  overrides: Partial<React.ComponentProps<typeof GameBrowser>> = {},
  view: LibraryView = "list",
) {
  return render(
    <TooltipProvider>
      <GameBrowser
        games={games}
        total={games.length}
        view={view}
        selected={new Set()}
        hasMore={false}
        loadingMore={false}
        onLoadMore={vi.fn()}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
        grouping="initial"
        {...overrides}
      />
    </TooltipProvider>,
  );
}

describe("índice de salto de la biblioteca", () => {
  beforeEach(() => {
    virtual.scrollOffset = 0;
    virtual.scrollToIndex.mockClear();
  });

  it("no aparece sin agrupación", () => {
    renderBrowser({ grouping: "none" });
    expect(screen.queryByRole("navigation", { name: "Índice de grupos" })).not.toBeInTheDocument();
  });

  it("no aparece con menos de tres grupos", () => {
    renderBrowser({ games: [game(1, "Abzû"), game(2, "Braid")], total: 2 });
    expect(screen.queryByRole("navigation", { name: "Índice de grupos" })).not.toBeInTheDocument();
  });

  it("lista un destino por grupo y marca aquel por el que va la lectura", () => {
    renderBrowser();
    const rail = screen.getByRole("navigation", { name: "Índice de grupos" });
    expect(Array.from(rail.querySelectorAll("button")).map((button) => button.textContent)).toEqual(
      ["A", "B", "C", "D"],
    );
    expect(screen.getByRole("button", { name: "Ir a A" })).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "Ir a C" })).not.toHaveAttribute("aria-current");
  });

  it("desplaza hasta la primera fila del grupo al activarlo", async () => {
    const user = userEvent.setup();
    renderBrowser();
    await user.click(screen.getByRole("button", { name: "Ir a C" }));
    // Encabezado A, dos juegos, encabezado B, un juego, encabezado C.
    expect(virtual.scrollToIndex).toHaveBeenCalledWith(5, { align: "start" });
  });

  it("recorre el raíl con las flechas y salta con Intro", async () => {
    const user = userEvent.setup();
    renderBrowser();
    screen.getByRole("button", { name: "Ir a A" }).focus();
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Ir a C" }));
    expect(virtual.scrollToIndex).not.toHaveBeenCalled();

    await user.keyboard("{Enter}");
    expect(virtual.scrollToIndex).toHaveBeenCalledWith(5, { align: "start" });
  });

  it("no se sale de los extremos del raíl", async () => {
    const user = userEvent.setup();
    renderBrowser();
    screen.getByRole("button", { name: "Ir a A" }).focus();
    await user.keyboard("{ArrowUp}");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Ir a A" }));
    await user.keyboard("{End}{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Ir a D" }));
  });
});

describe("encabezado de grupo fijado", () => {
  beforeEach(() => {
    virtual.scrollOffset = 0;
    virtual.scrollToIndex.mockClear();
  });

  it("se dibuja fuera del lienzo virtual y no duplica el del grupo activo", () => {
    const { container } = renderBrowser();
    const pinned = container.querySelector(".library-group-header--pinned");
    expect(pinned).toBeInTheDocument();
    expect(pinned?.closest(".virtual-canvas")).toBeNull();
    expect(pinned).toHaveTextContent("A");
    // El encabezado de «A» ya lo pinta la franja: dentro del lienzo quedan B, C y D.
    const inCanvas = container.querySelectorAll(".virtual-canvas .library-group-header");
    expect(Array.from(inCanvas).map((node) => node.getAttribute("data-group"))).toEqual([
      "B",
      "C",
      "D",
    ]);
  });

  it("cambia de grupo al entrar en el siguiente", () => {
    const { container, rerender } = renderBrowser();
    // Encabezado A (30) + dos filas de 58 = comienzo del encabezado B.
    virtual.scrollOffset = 30 + 58 * 2;
    rerender(
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
          grouping="initial"
        />
      </TooltipProvider>,
    );
    expect(container.querySelector(".library-group-header--pinned")).toHaveTextContent("B");
  });

  it("la rejilla fija su grupo con las mismas reglas que la lista", () => {
    const { container } = renderBrowser({}, "grid");
    expect(container.querySelector(".library-group-header--pinned")).toHaveTextContent("A");
    const inCanvas = container.querySelectorAll(".virtual-canvas .library-group-header");
    expect(Array.from(inCanvas).map((node) => node.getAttribute("data-group"))).toEqual([
      "B",
      "C",
      "D",
    ]);
  });
});

describe("recuento del encabezado de grupo", () => {
  beforeEach(() => {
    virtual.scrollOffset = 0;
  });

  it("dice cuántos lleva cargados mientras queden páginas por traer", () => {
    const { container } = renderBrowser({ hasMore: true, total: 1_500 });
    expect(container.querySelector(".library-group-header--pinned data")).toHaveTextContent(
      "2 cargados",
    );
  });

  it("da el número a secas solo cuando la página es la biblioteca entera", () => {
    const { container } = renderBrowser({ hasMore: false, total: games.length });
    const pinned = container.querySelector(".library-group-header--pinned data");
    expect(pinned).toHaveTextContent("2");
    expect(pinned).not.toHaveTextContent("cargados");
  });
});

describe("agrupación en rejilla", () => {
  beforeEach(() => {
    virtual.scrollOffset = 0;
  });

  it("corta la rejilla en grupos sin mezclar dos en la misma fila", () => {
    const { container } = renderBrowser({}, "grid");
    const rows = Array.from(container.querySelectorAll(".virtual-grid-row"));
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      const initials = new Set(
        Array.from(row.querySelectorAll<HTMLElement>(".game-card__title-row h3")).map((node) =>
          (node.textContent ?? "").charAt(0),
        ),
      );
      expect(initials.size).toBe(1);
    }
    // Cuatro iniciales: la de la franja fija más las tres que quedan en el lienzo.
    expect(container.querySelectorAll(".library-group-header")).toHaveLength(4);
  });
});

describe("banda muerta bajo la cabecera de columnas", () => {
  beforeEach(() => {
    virtual.scrollOffset = 0;
  });

  it.each(["list", "compact"] as const)(
    "la primera fila arranca pegada a la cabecera en la vista %s",
    (view) => {
      const { container } = renderBrowser({ grouping: "none" }, view);
      const canvas = container.querySelector<HTMLElement>(".virtual-canvas");
      // La cabecera es `sticky` y ya ocupa su alto en el flujo: si el lienzo
      // añadiera el suyo, entre ambos quedaría una franja vacía.
      expect(canvas?.style.marginTop).toBe("0px");
      const first = container.querySelector<HTMLElement>(".game-row");
      expect(first?.style.transform).toContain("translateY(0px)");
    },
  );
});
