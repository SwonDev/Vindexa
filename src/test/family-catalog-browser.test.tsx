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
  // La precarga es una mejora de tiempos: en pruebas basta con que exista.
  prefetchArtwork: () => undefined,
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

  it("distingue confirmación sin promover pendientes a la ficha personal", async () => {
    const user = userEvent.setup();
    const { props } = renderBrowser({ view: "list" });

    // Sólo el comprobado abre ficha: del otro no hay evidencia local, así que
    // ofrecer su ficha personal sería dar por hecho que se puede jugar, y lo
    // que hace es llevar a la tienda.
    await user.click(screen.getByRole("button", { name: "Abrir Confirmado" }));
    expect(props.onOpenConfirmed).toHaveBeenCalledWith(10);

    await user.click(screen.getByRole("button", { name: "Abrir Pendiente" }));
    expect(props.onOpenConfirmed).toHaveBeenCalledTimes(1);
  });

  it("marca sólo lo comprobado y no rotula el resto", () => {
    // Con mil ochocientos juegos prestados, rotular «por confirmar» en cada
    // carátula no informaba de nada: es el estado normal. Lo que merece marca es
    // la excepción, como «INSTALADO» en la biblioteca.
    const { container } = renderBrowser();

    const marcas = container.querySelectorAll(".installed-marker");
    expect(marcas).toHaveLength(1);
    expect(marcas[0]?.textContent).toContain("COMPROBADO");
    expect(screen.queryByText(/Por confirmar/)).toBeNull();
  });

  it("renderiza una fila densa por juego en vista ultracompacta", () => {
    const { container } = renderBrowser({ view: "compact" });

    expect(container.querySelector(".catalog-browser--compact")).toBeInTheDocument();
    expect(container.querySelectorAll(".catalog-row")).toHaveLength(2);
    expect(screen.getAllByTestId("artwork")).toHaveLength(2);
  });

  it.each([{ density: "compact" as const }, { density: "comfortable" as const }])(
    "reserva la altura completa de tres filas $density sin solapar tarjetas",
    ({ density }) => {
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
      // La altura concreta depende del ritmo de la retícula, que se ajusta; lo
      // que no puede cambiar es que las filas queden contiguas y sin solape.
      const offsets = rows.map((row) =>
        Number.parseFloat(row.style.transform.replace(/[^\d.-]/g, "")),
      );
      const [first, second, third] = offsets as [number, number, number];
      const step = second - first;
      expect(first).toBe(0);
      expect(step).toBeGreaterThan(0);
      expect(third).toBe(step * 2);
      expect(canvas?.style.height).toBe(`${step * 3}px`);
      // La altura de fila la fija ahora la misma geometría que la biblioteca:
      // el catálogo dejó de reservarse un cuerpo propio de ciento sesenta
      // píxeles para dos botones y dos párrafos que ya no están.
      expect(canvas?.style.getPropertyValue("--family-grid-body-height")).toBe("");
    },
  );

  it("vuelve al inicio al cambiar filtro u orden aunque el offset persistido ya sea cero", () => {
    const onScrollOffsetChange = vi.fn();
    const result = renderBrowser({ initialScrollOffset: 0, onScrollOffsetChange });
    const browser = result.container.querySelector<HTMLElement>(".catalog-browser");
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
    const browser = result.container.querySelector<HTMLElement>(".catalog-browser");
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
