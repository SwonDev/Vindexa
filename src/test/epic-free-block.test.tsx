import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EpicFreeBlock } from "@/features/discovery/EpicFreeBlock";
import { api } from "@/lib/tauri";
import type { EpicFreeOffer } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    epicFreeGames: vi.fn(),
    dismissEpicFreeGame: vi.fn(),
    openEpicFreeGame: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

function offer(overrides: Partial<EpicFreeOffer> & { offerId: string }): EpicFreeOffer {
  return {
    title: "Caravan SandWitch",
    description: "",
    storeUrl: "https://store.epicgames.com/es-ES/p/caravan-sandwitch",
    imageUrl: null,
    state: "current",
    startsAt: "2026-08-14T15:00:00Z",
    endsAt: "2026-08-21T15:00:00Z",
    originalPriceCents: 2499,
    currency: "EUR",
    owned: false,
    hoursLeft: 50,
    dismissed: false,
    ...overrides,
  };
}

function renderBlock() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <EpicFreeBlock />
    </QueryClientProvider>,
  );
}

/**
 * Fija el reloj para esta prueba.
 *
 * «Hoy» y «mañana» son relativos por definición: sin fijar el reloj, la misma
 * prueba pasa hoy y falla el jueves que viene. `shouldAdvanceTime` deja que
 * las esperas de la interfaz sigan corriendo.
 */
function conElRelojEn(iso: string) {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(new Date(iso));
}

afterEach(() => {
  vi.useRealTimers();
});

beforeEach(() => {
  vi.clearAllMocks();
  mockedApi.openEpicFreeGame.mockResolvedValue(undefined);
  mockedApi.dismissEpicFreeGame.mockResolvedValue(undefined);
});

/**
 * Los regalos de Epic, en pantalla.
 *
 * Lo que se comprueba aquí no es que el bloque exista, sino las cuatro cosas
 * que lo hacen útil: que diga cuánto queda, que no ofrezca reclamar lo que ya
 * tienes, que no invente una cuenta atrás que Epic no ha publicado y que
 * reclamar lleve a la ficha en el navegador integrado.
 */
describe("gratis en Epic", () => {
  it("enseña lo que se puede reclamar y cuánto queda", async () => {
    mockedApi.epicFreeGames.mockResolvedValue([offer({ offerId: "of-1" })]);
    renderBlock();

    expect(await screen.findByText("Caravan SandWitch")).toBeVisible();
    expect(screen.getByText("Quedan 2 días")).toBeVisible();
    // El precio de fuera de la promoción es lo que **no** se paga.
    expect(screen.getByText("24,99 €")).toBeVisible();
    expect(screen.getByRole("button", { name: /Reclamar/ })).toBeVisible();
  });

  it("lo que ya tienes se dice, y no se ofrece reclamar", async () => {
    // Es la única cosa que Vindexa sabe y la tienda no.
    mockedApi.epicFreeGames.mockResolvedValue([offer({ offerId: "of-1", owned: true })]);
    renderBlock();

    expect(await screen.findByText("Ya lo tienes")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Reclamar/ })).toBeNull();
  });

  it("sin fecha de fin no se inventa una cuenta atrás", async () => {
    mockedApi.epicFreeGames.mockResolvedValue([
      offer({ offerId: "of-1", endsAt: null, hoursLeft: null }),
    ]);
    renderBlock();

    expect(await screen.findByText("Gratis ahora")).toBeVisible();
    expect(screen.queryByText(/Quedan/)).toBeNull();
  });

  it("reclamar lleva a la ficha de Epic en el navegador integrado", async () => {
    const user = userEvent.setup();
    mockedApi.epicFreeGames.mockResolvedValue([offer({ offerId: "of-1" })]);
    renderBlock();

    await user.click(await screen.findByRole("button", { name: /Reclamar/ }));
    await waitFor(() =>
      expect(mockedApi.openEpicFreeGame).toHaveBeenCalledWith(
        "https://store.epicgames.com/es-ES/p/caravan-sandwitch",
      ),
    );
  });

  it("lo anunciado se enseña con su fecha, después de lo vigente", async () => {
    conElRelojEn("2026-08-18T09:00:00Z");
    mockedApi.epicFreeGames.mockResolvedValue([
      offer({
        offerId: "of-2",
        title: "Ghostrunner 2",
        state: "upcoming",
        startsAt: "2026-08-21T15:00:00Z",
        hoursLeft: null,
      }),
      offer({ offerId: "of-1" }),
    ]);
    renderBlock();

    const filas = await screen.findAllByRole("listitem");
    expect(filas[0]).toHaveTextContent("Caravan SandWitch");
    expect(filas[1]).toHaveTextContent("Ghostrunner 2");
    expect(within(filas[1] as HTMLElement).getByText(/Desde el/)).toBeVisible();
  });

  /**
   * Epic cambia el regalo los jueves a media tarde, así que lo anunciado
   * empieza **hoy** la mitad de las veces. «Desde el 20 ago» un 20 de agosto
   * obliga a mirar el calendario para entender que faltan horas, no días.
   */
  it("lo que empieza hoy lo dice, y a qué hora", async () => {
    conElRelojEn("2026-08-20T08:00:00Z");
    mockedApi.epicFreeGames.mockResolvedValue([
      offer({
        offerId: "of-3",
        title: "Cardpocalypse",
        state: "upcoming",
        startsAt: "2026-08-20T15:00:00Z",
        hoursLeft: null,
      }),
    ]);
    renderBlock();

    const fila = (await screen.findAllByRole("listitem"))[0] as HTMLElement;
    expect(within(fila).getByText(/^Hoy a las \d{2}:\d{2}$/)).toBeVisible();
    expect(within(fila).queryByText(/Desde el/)).toBeNull();
  });

  it("y lo de mañana se dice mañana, no «desde el 21»", async () => {
    conElRelojEn("2026-08-20T22:00:00Z");
    mockedApi.epicFreeGames.mockResolvedValue([
      offer({
        offerId: "of-4",
        title: "Paquete de Mago Épico",
        state: "upcoming",
        startsAt: "2026-08-21T15:00:00Z",
        hoursLeft: null,
      }),
    ]);
    renderBlock();

    const fila = (await screen.findAllByRole("listitem"))[0] as HTMLElement;
    expect(within(fila).getByText(/^Mañana a las \d{2}:\d{2}$/)).toBeVisible();
  });

  /**
   * Un regalo que terminó no es una oportunidad: es una página donde ya cuesta
   * dinero. Se sigue guardando —para no volver a avisar del mismo— pero no se
   * enseña.
   */
  it("lo que ya terminó no se enseña ni ofrece reclamarse", async () => {
    mockedApi.epicFreeGames.mockResolvedValue([
      offer({ offerId: "of-viejo", title: "Lo de la semana pasada", state: "expired" }),
    ]);
    const { container } = renderBlock();

    // Sin nada que enseñar, el bloque entero desaparece: un recuadro vacío en
    // una columna ya larga es ruido.
    await waitFor(() => expect(container.querySelector(".epic-free")).toBeNull());
    expect(screen.queryByText("Lo de la semana pasada")).toBeNull();
  });

  it("lo descartado no ocupa sitio", async () => {
    mockedApi.epicFreeGames.mockResolvedValue([offer({ offerId: "of-1", dismissed: true })]);
    const { container } = renderBlock();

    // Sin nada que enseñar, el bloque desaparece: un recuadro que dice «no hay
    // nada» en una columna ya larga es ruido.
    await waitFor(() => expect(container.querySelector(".epic-free")).toBeNull());
  });

  it("descartar se guarda", async () => {
    const user = userEvent.setup();
    mockedApi.epicFreeGames.mockResolvedValue([offer({ offerId: "of-1" })]);
    renderBlock();

    await user.pointer({
      keys: "[MouseRight]",
      target: await screen.findByText("Caravan SandWitch"),
    });
    await user.click(await screen.findByRole("menuitem", { name: "No me interesa" }));

    await waitFor(() => expect(mockedApi.dismissEpicFreeGame).toHaveBeenCalledWith("of-1"));
  });

  it("si Epic no contesta se dice, y no se finge que no hay regalos", async () => {
    mockedApi.epicFreeGames.mockRejectedValue(new Error("Epic no respondió."));
    renderBlock();

    expect(await screen.findByRole("alert")).toHaveTextContent("Epic no respondió.");
  });
});
