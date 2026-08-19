import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StoreDealsBlock } from "@/features/discovery/StoreDealsBlock";
import { api } from "@/lib/tauri";
import type { DealCandidate } from "@/lib/types";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div aria-hidden="true">{title}</div>,
  prefetchArtwork: () => undefined,
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    storeDeals: vi.fn(),
    dismissStoreDeal: vi.fn(),
    openStore: vi.fn(),
    saveWishlistEntry: vi.fn(),
    gamePreview: vi.fn(async () => ({ appId: 0, screenshots: [], checked: true })),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

function deal(overrides: Partial<DealCandidate> & { appId: number }): DealCandidate {
  return {
    title: "Una oferta",
    headerUrl: null,
    finalCents: 999,
    initialCents: 1999,
    discountPercent: 50,
    currency: "EUR",
    source: "specials",
    matchScore: 0.72,
    matchReason: "Coincide con tus 300 h en Acción",
    ...overrides,
  };
}

function renderBlock() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <StoreDealsBlock />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedApi.dismissStoreDeal.mockResolvedValue(undefined);
  mockedApi.openStore.mockResolvedValue(undefined);
  mockedApi.saveWishlistEntry.mockResolvedValue(undefined);
});

/**
 * Ofertas para ti.
 *
 * Lo que separa esta sección de un escaparate es la coincidencia con tu
 * historial. Estas pruebas comprueban precisamente eso: que se enseñe cuando se
 * ha podido calcular, que **no** se invente cuando no, y que las acciones que
 * ofrece hagan lo que dicen.
 */
describe("ofertas para ti", () => {
  it("enseña el precio, el descuento y la coincidencia", async () => {
    mockedApi.storeDeals.mockResolvedValue([deal({ appId: 10, title: "Kingdom Come" })]);
    renderBlock();

    const fila = await screen.findByRole("button", { name: /Kingdom Come/ });
    expect(within(fila).getByText("−50 %")).toBeVisible();
    expect(within(fila).getByText("9,99 €")).toBeVisible();
    expect(within(fila).getByText("72 %")).toBeVisible();
  });

  it("una oferta sin puntuar no finge un cero", async () => {
    // Sin sus géneros no se puede saber si encaja; cero significaría «no te
    // interesa», y eso no lo ha comprobado nadie.
    mockedApi.storeDeals.mockResolvedValue([
      deal({ appId: 10, title: "Sin puntuar", matchScore: null, matchReason: "" }),
    ]);
    renderBlock();

    const fila = await screen.findByRole("button", { name: /Sin puntuar/ });
    // La columna de coincidencia sencillamente no está; el descuento sí, que es
    // otro dato distinto y ese sí se sabe.
    expect(fila.querySelector(".store-deals__match")).toBeNull();
    expect(within(fila).getByText("−50 %")).toBeVisible();
  });

  it("cuenta cuántas encajan contigo, que es la razón de la sección", async () => {
    mockedApi.storeDeals.mockResolvedValue([
      deal({ appId: 10, matchScore: 0.8 }),
      deal({ appId: 11, matchScore: 0.2 }),
      deal({ appId: 12, matchScore: null }),
    ]);
    renderBlock();
    expect(await screen.findByText("1 encaja contigo")).toBeVisible();
  });

  it("pulsar una oferta abre su ficha en la tienda protegida", async () => {
    const user = userEvent.setup();
    mockedApi.storeDeals.mockResolvedValue([deal({ appId: 10, title: "Kingdom Come" })]);
    renderBlock();

    await user.click(await screen.findByRole("button", { name: /Kingdom Come/ }));
    await waitFor(() => expect(mockedApi.openStore).toHaveBeenCalledWith(10));
  });

  it("se puede pasar a deseados sin salir de aquí", async () => {
    const user = userEvent.setup();
    mockedApi.storeDeals.mockResolvedValue([deal({ appId: 10, title: "Kingdom Come" })]);
    renderBlock();

    await user.pointer({
      keys: "[MouseRight]",
      target: await screen.findByRole("button", { name: /Kingdom Come/ }),
    });
    await user.click(await screen.findByRole("menuitem", { name: "Añadir a deseados" }));

    await waitFor(() =>
      expect(mockedApi.saveWishlistEntry).toHaveBeenCalledWith(
        expect.objectContaining({ appId: 10, bucket: "waiting_sale" }),
      ),
    );
  });

  it("descartar una oferta se guarda", async () => {
    const user = userEvent.setup();
    mockedApi.storeDeals.mockResolvedValue([deal({ appId: 10, title: "Kingdom Come" })]);
    renderBlock();

    await user.pointer({
      keys: "[MouseRight]",
      target: await screen.findByRole("button", { name: /Kingdom Come/ }),
    });
    await user.click(await screen.findByRole("menuitem", { name: "No me interesa" }));
    await waitFor(() => expect(mockedApi.dismissStoreDeal).toHaveBeenCalledWith(10));
  });

  it("sin ofertas el bloque no ocupa sitio", async () => {
    mockedApi.storeDeals.mockResolvedValue([]);
    const { container } = renderBlock();
    await waitFor(() => expect(container.querySelector(".store-deals")).toBeNull());
  });

  it("si no se pueden leer se dice, en vez de parecer que no hay ninguna", async () => {
    mockedApi.storeDeals.mockRejectedValue(new Error("La tienda no respondió."));
    renderBlock();
    expect(await screen.findByRole("alert")).toHaveTextContent("La tienda no respondió.");
  });
});
