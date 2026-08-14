import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { FamilyCatalogBrowser } from "@/features/library/FamilyCatalogBrowser";
import { InterfaceDensityContext } from "@/features/shell/interface-density";
import type { FamilyCatalogGame } from "@/lib/types";

const openStore = vi.hoisted(() => vi.fn());
const measureVirtualizer = vi.hoisted(() => vi.fn());
const measureVirtualRow = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  api: { openStore },
  getErrorMessage: (error: unknown) => (error instanceof Error ? error.message : "Error"),
}));

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <span data-testid="artwork">{title}</span>,
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({
    count,
    estimateSize,
  }: {
    count: number;
    estimateSize: (index: number) => number;
  }) => {
    const rowSize = estimateSize(0);
    return {
      getVirtualItems: () =>
        Array.from({ length: count }, (_, index) => ({
          key: index,
          index,
          start: index * rowSize,
          size: rowSize,
          end: (index + 1) * rowSize,
          lane: 0,
        })),
      getTotalSize: () => count * rowSize,
      measure: measureVirtualizer,
      measureElement: measureVirtualRow,
    };
  },
}));

const games: FamilyCatalogGame[] = [
  {
    appId: 10,
    title: "Confirmado",
    availability: "confirmed",
    discoveredAt: "2026-08-10T10:00:00Z",
    updatedAt: "2026-08-14T10:00:00Z",
  },
  {
    appId: 20,
    title: "Pendiente",
    availability: "unknown",
    discoveredAt: "2026-08-11T10:00:00Z",
    updatedAt: "2026-08-13T10:00:00Z",
  },
];

function renderBrowser(
  overrides: Partial<React.ComponentProps<typeof FamilyCatalogBrowser>> = {},
  density: "compact" | "comfortable" = "compact",
) {
  const props: React.ComponentProps<typeof FamilyCatalogBrowser> = {
    games,
    total: games.length,
    view: "grid",
    availability: "all",
    sort: "availability",
    queryKey: "",
    hasMore: false,
    loadingMore: false,
    onAvailabilityChange: vi.fn(),
    onSortChange: vi.fn(),
    onViewChange: vi.fn(),
    onLoadMore: vi.fn(),
    onOpenConfirmed: vi.fn(),
    ...overrides,
  };
  return {
    ...render(
      <InterfaceDensityContext value={density}>
        <TooltipProvider>
          <FamilyCatalogBrowser {...props} />
        </TooltipProvider>
      </InterfaceDensityContext>,
    ),
    props,
  };
}

describe("catálogo familiar Steam-like", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    openStore.mockResolvedValue(undefined);
  });

  it("expone filtro, orden y las tres vistas como controles accesibles", async () => {
    const user = userEvent.setup();
    const { props } = renderBrowser();

    await user.click(screen.getByRole("combobox", { name: "Filtrar catálogo familiar" }));
    await user.click(screen.getByRole("option", { name: "Confirmados localmente" }));
    expect(props.onAvailabilityChange).toHaveBeenCalledWith("confirmed");

    await user.click(screen.getByRole("combobox", { name: "Ordenar catálogo familiar" }));
    await user.click(screen.getByRole("option", { name: "Actualizados recientemente" }));
    expect(props.onSortChange).toHaveBeenCalledWith("updatedDesc");

    expect(screen.getByRole("button", { name: "Vista familiar de cuadrícula" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Vista familiar ultracompacta" }));
    expect(props.onViewChange).toHaveBeenCalledWith("compact");
  });

  it("distingue confirmación sin promover pendientes a la ficha personal", async () => {
    const user = userEvent.setup();
    const { props } = renderBrowser({ view: "list" });

    expect(screen.getByText("2 juegos del grupo")).toBeVisible();
    expect(screen.getByRole("button", { name: "Abrir ficha de Confirmado" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Abrir ficha de Pendiente" })).toBeNull();
    expect(screen.getByText("Por confirmar")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Abrir ficha de Confirmado" }));
    expect(props.onOpenConfirmed).toHaveBeenCalledWith(10);
  });

  it("renderiza una fila densa por juego en vista ultracompacta", () => {
    const { container } = renderBrowser({ view: "compact" });

    expect(container.querySelector(".family-catalog-browser--compact")).toBeInTheDocument();
    expect(container.querySelectorAll(".family-game-row")).toHaveLength(2);
    expect(screen.getAllByTestId("artwork")).toHaveLength(2);
  });

  it.each([
    { density: "compact" as const, bodyHeight: 156, rowHeight: 426 },
    { density: "comfortable" as const, bodyHeight: 166, rowHeight: 436 },
  ])(
    "reserva la altura completa de tres filas $density sin solapar tarjetas",
    ({ density, bodyHeight, rowHeight }) => {
      const manyGames = Array.from({ length: 11 }, (_, index) => ({
        ...games[index % games.length],
        appId: 100 + index,
        title: `Juego ${index + 1}`,
      }));
      const { container } = renderBrowser({ games: manyGames, total: manyGames.length }, density);

      const canvas = container.querySelector<HTMLElement>(".virtual-canvas");
      const rows = Array.from(container.querySelectorAll<HTMLElement>(".virtual-grid-row"));

      expect(rows).toHaveLength(3);
      expect(rows.map((row) => row.dataset.index)).toEqual(["0", "1", "2"]);
      expect(rows.map((row) => row.style.transform)).toEqual([
        "translateY(0px)",
        `translateY(${rowHeight}px)`,
        `translateY(${rowHeight * 2}px)`,
      ]);
      expect(canvas?.style.height).toBe(`${rowHeight * 3}px`);
      expect(canvas?.style.getPropertyValue("--family-grid-body-height")).toBe(`${bodyHeight}px`);
      expect(canvas?.style.getPropertyValue("--family-grid-row-gap")).toBe(
        density === "compact" ? "10px" : "14px",
      );
    },
  );

  it("vuelve al inicio al cambiar filtro u orden aunque el offset persistido ya sea cero", () => {
    const onScrollOffsetChange = vi.fn();
    const result = renderBrowser({ initialScrollOffset: 0, onScrollOffsetChange });
    const browser = result.container.querySelector<HTMLElement>(".family-catalog-browser");
    expect(browser).not.toBeNull();
    if (!browser) return;

    browser.scrollTop = 180;
    fireEvent.scroll(browser);
    expect(onScrollOffsetChange).toHaveBeenLastCalledWith(180);

    result.rerender(
      <InterfaceDensityContext value="compact">
        <TooltipProvider>
          <FamilyCatalogBrowser {...result.props} sort="alphabetical" />
        </TooltipProvider>
      </InterfaceDensityContext>,
    );

    expect(browser.scrollTop).toBe(0);
  });

  it("vuelve al inicio cuando cambia la búsqueda efectiva", () => {
    const result = renderBrowser({ initialScrollOffset: 0 });
    const browser = result.container.querySelector<HTMLElement>(".family-catalog-browser");
    expect(browser).not.toBeNull();
    if (!browser) return;
    browser.scrollTop = 220;

    result.rerender(
      <InterfaceDensityContext value="compact">
        <TooltipProvider>
          <FamilyCatalogBrowser {...result.props} queryKey="hades" />
        </TooltipProvider>
      </InterfaceDensityContext>,
    );

    expect(browser.scrollTop).toBe(0);
  });

  it("remide la virtualización al cambiar vista, densidad o ancho con el mismo catálogo", () => {
    const OriginalResizeObserver = window.ResizeObserver;
    let resizeCallback: ResizeObserverCallback | undefined;
    class ControllableResizeObserver implements ResizeObserver {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    window.ResizeObserver = ControllableResizeObserver;

    try {
      const manyGames = Array.from({ length: 11 }, (_, index) => ({
        ...games[index % games.length],
        appId: 300 + index,
        title: `Catálogo ${index + 1}`,
      }));
      const result = renderBrowser({ games: manyGames, total: manyGames.length });
      measureVirtualizer.mockClear();

      result.rerender(
        <InterfaceDensityContext value="compact">
          <TooltipProvider>
            <FamilyCatalogBrowser {...result.props} view="list" />
          </TooltipProvider>
        </InterfaceDensityContext>,
      );
      expect(measureVirtualizer).toHaveBeenCalled();
      measureVirtualizer.mockClear();

      result.rerender(
        <InterfaceDensityContext value="comfortable">
          <TooltipProvider>
            <FamilyCatalogBrowser {...result.props} view="list" />
          </TooltipProvider>
        </InterfaceDensityContext>,
      );
      expect(measureVirtualizer).toHaveBeenCalled();
      measureVirtualizer.mockClear();

      result.rerender(
        <InterfaceDensityContext value="compact">
          <TooltipProvider>
            <FamilyCatalogBrowser {...result.props} />
          </TooltipProvider>
        </InterfaceDensityContext>,
      );
      measureVirtualizer.mockClear();
      act(() => {
        resizeCallback?.(
          [{ contentRect: { width: 880 } } as unknown as ResizeObserverEntry],
          {} as ResizeObserver,
        );
      });
      expect(result.container.querySelectorAll(".virtual-grid-row")).toHaveLength(3);
      expect(measureVirtualizer).toHaveBeenCalled();
    } finally {
      window.ResizeObserver = OriginalResizeObserver;
    }
  });
});
