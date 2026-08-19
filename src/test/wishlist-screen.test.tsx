import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { WishlistScreen } from "@/features/wishlist/WishlistScreen";
import { api } from "@/lib/tauri";
import type { GameSummary, WishlistBucketId, WishlistEntry, WishlistOverview } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    wishlistOverview: vi.fn(),
    wishlistPrices: vi.fn(),
    refreshWishlistPrices: vi.fn(),
    saveWishlistEntry: vi.fn(),
    removeWishlistEntry: vi.fn(),
    openStore: vi.fn(),
    moveWishlistEntry: vi.fn(),
    reorderWishlistBucket: vi.fn(),
    listGameVideos: vi.fn(),
    saveGameVideo: vi.fn(),
    deleteGameVideo: vi.fn(),
    reorderGameVideos: vi.fn(),
    listGames: vi.fn(),
    listCuratedLists: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

// La carátula real pide arte a Tauri. Aquí solo interesa que exista un hueco:
// el mock no repite el título para que las consultas por texto no encuentren
// dos veces el mismo juego.
vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <div data-artwork={title} aria-hidden="true" />,
  // La precarga es una mejora de tiempos: en pruebas basta con que exista.
  prefetchArtwork: () => undefined,
}));

const mockedApi = api as unknown as { [Key in keyof typeof api]: ReturnType<typeof vi.fn> };

function game(appId: number, title: string): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: false,
    statusId: "backlog",
    statusName: "Pendiente",
    statusColor: "#5CAAC1",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 0,
    collectionIds: [],
  };
}

function entry(
  appId: number,
  title: string,
  bucket: WishlistBucketId,
  position: number,
  extra: Partial<WishlistEntry> = {},
): WishlistEntry {
  return {
    game: game(appId, title),
    bucket,
    priority: 0,
    position,
    note: "",
    addedAt: "2026-08-01T10:00:00Z",
    updatedAt: "2026-08-01T10:00:00Z",
    ...extra,
  };
}

const silksong = entry(1030300, "Hollow Knight: Silksong", "buying_now", 0, {
  priority: 5,
  targetPriceCents: 2999,
  currency: "EUR",
  note: "Doce años esperando.",
});
const hades = entry(1145360, "Hades II", "waiting_sale", 0, {
  priority: 3,
  targetPriceCents: 1999,
  currency: "EUR",
});
const outer = entry(1621690, "Outer Wilds: Echoes", "waiting_sale", 1, {
  priority: 2,
  targetPriceCents: 1500,
  currency: "USD",
});
const dredge = entry(1562430, "Dredge", "considering", 0);

const fullOverview: WishlistOverview = {
  buckets: [
    { bucket: "buying_now", items: [silksong], total: 1 },
    { bucket: "waiting_sale", items: [hades, outer], total: 2 },
    { bucket: "considering", items: [dredge], total: 1 },
    { bucket: "watching", items: [], total: 0 },
  ],
  total: 4,
  targetTotals: [
    { currency: "EUR", totalCents: 4998, entries: 2 },
    { currency: "USD", totalCents: 1500, entries: 1 },
  ],
  entriesWithoutTarget: 1,
};

const emptyOverview: WishlistOverview = {
  buckets: [],
  total: 0,
  targetTotals: [],
  entriesWithoutTarget: 0,
};

function renderScreen() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <WishlistScreen loading={false} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function lane(name: string): HTMLElement {
  return screen.getByRole("region", { name: new RegExp(`^${name}:`) });
}

describe("pantalla de deseados · cubos de intención", () => {
  beforeEach(() => {
    mockedApi.wishlistOverview.mockResolvedValue(fullOverview);
    // Sin precios observados: es el estado normal de una lista recién creada y
    // el que obliga a la pantalla a decir que no se sabe.
    mockedApi.wishlistPrices.mockResolvedValue([]);
    mockedApi.listGameVideos.mockResolvedValue([]);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
    mockedApi.saveWishlistEntry.mockImplementation((input: { appId: number }) =>
      Promise.resolve({ ...silksong, game: game(input.appId, "Guardado") }),
    );
    mockedApi.moveWishlistEntry.mockResolvedValue(undefined);
    mockedApi.reorderWishlistBucket.mockResolvedValue(undefined);
    mockedApi.removeWishlistEntry.mockResolvedValue(undefined);
  });

  it("reparte los deseados en los cuatro carriles, incluido el que está vacío", async () => {
    renderScreen();

    expect(await screen.findByRole("button", { name: "Hollow Knight: Silksong" })).toBeVisible();
    expect(within(lane("Comprar ya")).getByText("Hollow Knight: Silksong")).toBeVisible();
    expect(within(lane("Esperando oferta")).getByText("Hades II")).toBeVisible();
    expect(within(lane("Esperando oferta")).getByText("Outer Wilds: Echoes")).toBeVisible();
    expect(within(lane("Considerando")).getByText("Dredge")).toBeVisible();
    // El carril vacío no desaparece: si lo hiciera, el destino del arrastre
    // cambiaría a mitad del gesto.
    expect(within(lane("Vigilando")).getByText(/Para lo que sigues de reojo/)).toBeVisible();
  });

  it("presenta el agregado como un suelo cuando hay entradas sin precio y jamás suma monedas", async () => {
    renderScreen();

    const figure = await screen.findByText(/^Al menos/);
    expect(figure.textContent).toContain("49,98");
    expect(figure.textContent).toContain("15,00");
    // La suma prohibida entre monedas distintas.
    expect(figure.textContent).not.toContain("64,98");
    expect(figure).toHaveAttribute("data-at-least", "true");

    expect(screen.getByText(/1 entrada sin precio objetivo/)).toBeVisible();
    expect(screen.getByText(/nunca sumadas entre sí/)).toBeVisible();
  });

  it("con todas las entradas valoradas la cifra deja de ser un mínimo", async () => {
    mockedApi.wishlistOverview.mockResolvedValue({
      ...fullOverview,
      entriesWithoutTarget: 0,
      targetTotals: [{ currency: "EUR", totalCents: 4998, entries: 4 }],
    });
    renderScreen();

    await screen.findByRole("button", { name: "Hollow Knight: Silksong" });
    expect(screen.queryByText(/^Al menos/)).toBeNull();
    expect(screen.getByText("49,98 €")).toHaveAttribute("data-at-least", "false");
  });

  it("no dibuja ningún asa sobre la carátula: el activador es solo para teclado", async () => {
    renderScreen();
    await screen.findByRole("button", { name: "Hollow Knight: Silksong" });

    const activator = screen.getByRole("button", { name: "Arrastrar Hollow Knight: Silksong" });
    expect(activator).toHaveClass("wishlist-drag-activator");
    expect(activator).toBeEmptyDOMElement();
    expect(document.querySelector(".wishlist-drag-handle")).toBeNull();
  });

  it("levanta el juego desde cualquier punto de la tarjeta, no desde un asa", async () => {
    renderScreen();
    const target = await screen.findByRole("button", { name: "Hollow Knight: Silksong" });

    fireEvent.pointerDown(target, {
      button: 0,
      isPrimary: true,
      pointerId: 1,
      clientX: 20,
      clientY: 20,
    });
    fireEvent.pointerMove(document, { pointerId: 1, clientX: 20, clientY: 90 });

    await waitFor(() =>
      expect(document.querySelector('[data-dragging="true"]')).toBeInTheDocument(),
    );
    expect(document.querySelector(".wishlist-drag-ghost")).toBeInTheDocument();

    fireEvent.pointerUp(document, { pointerId: 1 });
  });

  it("ofrece una alternativa completa al arrastre: reordenar y cambiar de carril con teclado", async () => {
    const user = userEvent.setup();
    renderScreen();
    await screen.findByRole("button", { name: "Hades II" });

    await user.click(screen.getByRole("button", { name: "Bajar Hades II" }));
    await waitFor(() =>
      expect(mockedApi.reorderWishlistBucket).toHaveBeenCalledWith(
        "waiting_sale",
        [1621690, 1145360],
      ),
    );

    await user.click(screen.getByRole("button", { name: "Mover Hades II a otro carril" }));
    await user.click(await screen.findByRole("menuitem", { name: "Comprar ya" }));
    await waitFor(() =>
      expect(mockedApi.moveWishlistEntry).toHaveBeenCalledWith(1145360, "buying_now", undefined),
    );
  });

  it("devuelve la tarjeta a su sitio y lo dice cuando el backend rechaza el movimiento", async () => {
    const user = userEvent.setup();
    mockedApi.reorderWishlistBucket.mockRejectedValue(
      new Error("La base de datos está bloqueada."),
    );
    renderScreen();
    await screen.findByRole("button", { name: "Hades II" });

    await user.click(screen.getByRole("button", { name: "Bajar Hades II" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No se pudo reordenar: La base de datos está bloqueada.",
    );
    const waiting = lane("Esperando oferta");
    const titles = within(waiting)
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent);
    expect(titles).toEqual(["Hades II", "Outer Wilds: Echoes"]);
  });

  it("guarda prioridad, precio objetivo y nota de la entrada seleccionada", async () => {
    const user = userEvent.setup();
    renderScreen();
    await screen.findByRole("button", { name: "Hollow Knight: Silksong" });

    const price = screen.getByLabelText("Precio objetivo");
    await user.clear(price);
    await user.type(price, "24,50");
    const note = screen.getByLabelText("Nota");
    await user.clear(note);
    await user.type(note, "Solo si baja de 25.");
    await user.click(screen.getByRole("radio", { name: "4" }));
    await user.click(screen.getByRole("button", { name: /Guardar plan/ }));

    await waitFor(() =>
      expect(mockedApi.saveWishlistEntry).toHaveBeenCalledWith({
        appId: 1030300,
        bucket: "buying_now",
        priority: 4,
        note: "Solo si baja de 25.",
        targetPriceCents: 2450,
        currency: "EUR",
      }),
    );
  });

  it("rechaza un precio que no es un importe sin llamar al backend", async () => {
    const user = userEvent.setup();
    renderScreen();
    await screen.findByRole("button", { name: "Hollow Knight: Silksong" });

    const price = screen.getByLabelText("Precio objetivo");
    await user.clear(price);
    await user.type(price, "cuando esté de oferta");
    await user.click(screen.getByRole("button", { name: /Guardar plan/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Escribe una cantidad con hasta dos decimales",
    );
    expect(mockedApi.saveWishlistEntry).not.toHaveBeenCalled();
    expect(price).toHaveAttribute("aria-invalid", "true");
  });

  it("añade un juego al carril desde el buscador de la biblioteca", async () => {
    const user = userEvent.setup();
    mockedApi.listGames.mockResolvedValue({
      items: [game(413150, "Stardew Valley")],
      total: 1,
      limit: 8,
      offset: 0,
    });
    renderScreen();
    await screen.findByRole("button", { name: "Hollow Knight: Silksong" });

    await user.click(screen.getByRole("button", { name: "Añadir un juego a Considerando" }));
    await user.type(screen.getByLabelText("Añadir a Considerando"), "stardew");

    await user.click(await screen.findByRole("button", { name: /Stardew Valley/ }));

    await waitFor(() =>
      expect(mockedApi.saveWishlistEntry).toHaveBeenCalledWith({
        appId: 413150,
        bucket: "considering",
        priority: 0,
        note: "",
      }),
    );
  });
});

describe("pantalla de deseados · estados límite", () => {
  beforeEach(() => {
    mockedApi.listGameVideos.mockResolvedValue([]);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
  });

  it("sin deseados dice qué aporta cada carril en vez de enseñar cuatro cajas vacías", async () => {
    mockedApi.wishlistOverview.mockResolvedValue(emptyOverview);
    renderScreen();

    expect(await screen.findByText(/Nada decidido todavía/)).toBeVisible();
    expect(screen.getByText(/solo esperan un descuento/)).toBeVisible();
    expect(screen.getByText("Los deseados están vacíos")).toBeVisible();
    // Sin nada en la lista no hay cifra que dar: el encabezado se calla y el
    // estado vacío es el que explica para qué sirve cada carril.
    expect(screen.queryByText("Sin precio objetivo")).toBeNull();
    expect(document.querySelector(".wishlist-heading__figure")).toBeNull();
  });

  it("cuando la lectura falla ofrece reintentar en lugar de una pantalla en blanco", async () => {
    mockedApi.wishlistOverview.mockRejectedValue(new Error("SQLite no responde."));
    renderScreen();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("No se pudieron leer los deseados");
    expect(alert).toHaveTextContent("SQLite no responde.");
    expect(within(alert).getByRole("button", { name: "Reintentar" })).toBeVisible();
  });
});

/**
 * El clic derecho sobre una tarjeta de deseados.
 *
 * Los botones de la tarjeta sólo aparecen al pasar por encima. Además, abrir la
 * tienda de un deseado no estaba en ninguna parte: había que buscar el juego a
 * mano fuera de Vindexa.
 */
describe("acciones rápidas de un deseado", () => {
  beforeEach(() => {
    mockedApi.wishlistOverview.mockResolvedValue(fullOverview);
    mockedApi.wishlistPrices.mockResolvedValue([]);
    mockedApi.removeWishlistEntry.mockResolvedValue(undefined);
    mockedApi.openStore.mockResolvedValue(undefined);
  });

  it("ofrece editar, abrir la tienda, mover y quitar", async () => {
    const user = userEvent.setup();
    renderScreen();

    const tarjeta = await screen.findByRole("button", { name: "Hollow Knight: Silksong" });
    await user.pointer({ keys: "[MouseRight]", target: tarjeta });

    const menu = await screen.findByRole("menu", {
      name: /Acciones rápidas de Hollow Knight: Silksong/,
    });
    expect(
      within(menu).getByRole("menuitem", { name: "Editar nota y precio objetivo" }),
    ).toBeVisible();
    expect(
      within(menu).getByRole("menuitem", { name: "Abrir en la tienda oficial" }),
    ).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Mover a" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Quitar de los deseados" })).toBeVisible();
  });

  it("abre la tienda oficial, que no estaba en ningún otro sitio", async () => {
    const user = userEvent.setup();
    renderScreen();

    await user.pointer({
      keys: "[MouseRight]",
      target: await screen.findByRole("button", { name: "Hollow Knight: Silksong" }),
    });
    await user.click(await screen.findByRole("menuitem", { name: "Abrir en la tienda oficial" }));

    await waitFor(() => expect(mockedApi.openStore).toHaveBeenCalledWith(1030300));
  });

  it("subir está apagado en el primero de su carril", async () => {
    // Ofrecer un movimiento imposible es peor que no ofrecerlo: parece que la
    // aplicación ignora la orden.
    const user = userEvent.setup();
    renderScreen();

    await user.pointer({
      keys: "[MouseRight]",
      target: await screen.findByRole("button", { name: "Hollow Knight: Silksong" }),
    });
    const menu = await screen.findByRole("menu", { name: /Acciones rápidas/ });
    expect(within(menu).getByRole("menuitem", { name: /Subir en el carril/ })).toHaveAttribute(
      "data-disabled",
    );
  });
});
