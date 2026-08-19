import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { WishlistList } from "@/features/wishlist/WishlistList";
import type { WishlistEntry, WishlistOverview, WishlistPriceStatus } from "@/lib/types";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div aria-hidden="true">{title}</div>,
  prefetchArtwork: () => undefined,
}));

vi.mock("@/lib/tauri", () => ({
  api: { gamePreview: vi.fn(async () => ({ appId: 0, screenshots: [], checked: true })) },
  getErrorMessage: () => "No se pudo completar la operación.",
}));

// El virtualizador de listas no mide nada en jsdom: sin altura real devolvería
// cero filas y ninguna prueba vería nada. Se sustituye por uno que pinta todo.
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        key: index,
        index,
        start: index * 34,
        size: 34,
        end: index * 34 + 34,
        lane: 0,
      })),
    getTotalSize: () => count * 34,
    measure: vi.fn(),
    scrollToIndex: vi.fn(),
    scrollOffset: 0,
  }),
}));

function entry(
  appId: number,
  title: string,
  bucket: WishlistEntry["bucket"] = "considering",
): WishlistEntry {
  return {
    game: { appId, title, inLibrary: false },
    bucket,
    priority: 0,
    position: 0,
    note: "",
    addedAt: `2026-08-${String(appId).padStart(2, "0")}T10:00:00Z`,
    updatedAt: "2026-08-19T10:00:00Z",
  };
}

function overview(entries: WishlistEntry[]): WishlistOverview {
  return {
    buckets: [
      { bucket: "buying_now", items: entries.filter((e) => e.bucket === "buying_now"), total: 0 },
      {
        bucket: "waiting_sale",
        items: entries.filter((e) => e.bucket === "waiting_sale"),
        total: 0,
      },
      { bucket: "considering", items: entries.filter((e) => e.bucket === "considering"), total: 0 },
      { bucket: "watching", items: entries.filter((e) => e.bucket === "watching"), total: 0 },
    ],
    total: entries.length,
    targetTotals: [],
    entriesWithoutTarget: entries.length,
  };
}

function price(
  appId: number,
  finalCents: number,
  discountPercent = 0,
  initialCents = finalCents,
): WishlistPriceStatus {
  return {
    appId,
    otherCurrencies: [],
    comparable: false,
    meetsTarget: false,
    price: {
      appId,
      currency: "EUR",
      finalCents,
      initialCents,
      discountPercent,
      lowestCents: finalCents,
      lowestObservedAt: "2026-08-19T10:00:00Z",
      changedAt: "2026-08-19T10:00:00Z",
      observedAt: "2026-08-19T10:00:00Z",
      source: "steam_store",
      freshness: "fresh",
    },
  };
}

function renderList(entries: WishlistEntry[], prices: WishlistPriceStatus[] = []) {
  return render(
    <TooltipProvider>
      <WishlistList overview={overview(entries)} prices={prices} onSelect={vi.fn()} />
    </TooltipProvider>,
  );
}

/**
 * La lista de deseados.
 *
 * Lo que se comprueba es lo que la hacía inservible con mil cuatrocientos
 * juegos: que se pueda buscar y filtrar, que ordene por lo que se mira —el
 * descuento—, que el recuento coincida con lo que hay debajo, y que un precio
 * desconocido ocupe una palabra en vez de cuatro frases.
 */
describe("deseados en lista", () => {
  it("enseña una fila por juego con su precio y su descuento", () => {
    renderList(
      [entry(1, "Con oferta"), entry(2, "Sin oferta")],
      [price(1, 999, 60, 2499), price(2, 1999)],
    );

    const conOferta = screen.getByRole("button", { name: /Con oferta/ });
    expect(within(conOferta).getByText("−60 %")).toBeVisible();
    expect(within(conOferta).getByText("9,99 €")).toBeVisible();
    expect(within(conOferta).getByText("24,99 €")).toBeVisible();
  });

  it("un precio que no se sabe se dice una vez, no cuatro", () => {
    // Antes cada tarjeta escribía «Sin precio objetivo», «Precio desconocido»,
    // «Sin consultar» y «Sin precio objetivo ni precio consultado»: cuatro
    // líneas para decir que no se sabe nada, más altas que el propio título.
    renderList([entry(1, "Sin consultar")]);

    const fila = screen.getByRole("button", { name: /Sin consultar/ });
    expect(within(fila).getByText("sin precio")).toBeVisible();
    expect(within(fila).queryByText(/Precio desconocido/)).toBeNull();
    expect(within(fila).queryByText(/Sin precio objetivo/)).toBeNull();
  });

  it("ordena por descuento y deja al final lo que no se sabe", () => {
    renderList(
      [entry(1, "Poco"), entry(2, "Mucho"), entry(3, "Nada sabido")],
      [price(1, 900, 20, 1200), price(2, 500, 75, 2000)],
    );

    const titulos = screen
      .getAllByRole("button")
      .map((fila) => fila.textContent ?? "")
      .filter(
        (texto) => texto.includes("Poco") || texto.includes("Mucho") || texto.includes("Nada"),
      );
    expect(titulos[0]).toContain("Mucho");
    expect(titulos[1]).toContain("Poco");
    expect(titulos[2]).toContain("Nada sabido");
  });

  it("buscar recorta la lista y el recuento lo dice", async () => {
    const user = userEvent.setup();
    renderList([entry(1, "Hollow Knight"), entry(2, "Celeste"), entry(3, "Hades")]);

    expect(screen.getByRole("status")).toHaveTextContent("3 juegos");
    await user.type(screen.getByLabelText("Buscar en deseados"), "hollow");

    // El recuento nunca puede contradecir a la lista que tiene debajo.
    expect(screen.getByRole("status")).toHaveTextContent("1 de 3");
    expect(screen.queryByRole("button", { name: /Celeste/ })).toBeNull();
  });

  it("filtrar por carril enseña sólo ese carril", async () => {
    const user = userEvent.setup();
    renderList([entry(1, "Decidido", "buying_now"), entry(2, "Pensándolo", "considering")]);

    await user.click(screen.getByRole("radio", { name: /Comprar ya/ }));
    expect(screen.getByRole("button", { name: /Decidido/ })).toBeVisible();
    expect(screen.queryByRole("button", { name: /Pensándolo/ })).toBeNull();
  });

  it("dice cuántos están en oferta, que es la razón de mirar la lista", () => {
    renderList(
      [entry(1, "Uno"), entry(2, "Dos"), entry(3, "Tres")],
      [price(1, 500, 50, 1000), price(2, 800, 20, 1000), price(3, 1000)],
    );
    expect(screen.getByRole("status")).toHaveTextContent("2 en oferta");
  });

  it("la cobertura de precios va en la misma línea que el recuento", async () => {
    // Eran dos barras, una encima de otra, con dos cifras del mismo tipo: se
    // leían como dos cosas distintas diciendo lo mismo.
    render(
      <TooltipProvider>
        <WishlistList
          overview={overview([entry(1, "Uno"), entry(2, "Dos")])}
          prices={[price(1, 999)]}
          coverage={{ headline: "1 de 2 con precio", caveat: "1 juego sin precio consultado." }}
          onRefreshPrices={vi.fn()}
          onSelect={vi.fn()}
        />
      </TooltipProvider>,
    );

    const recuento = screen.getByRole("status");
    expect(recuento).toHaveTextContent("2 juegos");
    expect(recuento).toHaveTextContent("1 de 2 con precio");
    expect(within(recuento).getByRole("button", { name: /Actualizar precios/ })).toBeVisible();
  });

  it("sin coincidencias lo dice, en vez de dejar el hueco en blanco", async () => {
    const user = userEvent.setup();
    renderList([entry(1, "Hollow Knight")]);

    await user.type(screen.getByLabelText("Buscar en deseados"), "zzzz");
    expect(screen.getByText(/Ningún juego de este carril coincide/)).toBeVisible();
  });
});
