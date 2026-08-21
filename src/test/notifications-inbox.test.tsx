import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { NotificationsPopover } from "@/features/notifications/NotificationsPopover";
import { api } from "@/lib/tauri";
import type { NotificationEvent, NotificationInbox } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    notificationInbox: vi.fn(),
    listNotificationRules: vi.fn(),
    markNotificationRead: vi.fn(),
    markAllNotificationsRead: vi.fn(),
    dismissNotification: vi.fn(),
    dismissAllNotifications: vi.fn(),
    refreshNotificationEvents: vi.fn(),
    saveNotificationRule: vi.fn(),
    deleteNotificationRule: vi.fn(),
    listGames: vi.fn(),
    epicFreeGames: vi.fn(),
    openEpicFreeGame: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

/** Un anuncio de Steam de verdad: el cuerpo entero, sin recortar en origen. */
const ANUNCIO_LARGO =
  "Hello, this is the DragonSword : Awakening team. Thank you to everyone who joined our All Day " +
  "Live broadcast on August 10. We've compiled all questions received during the 8-hour stream " +
  "along with answers from Producer JYJ and PM Hooon. Watch the Replay. Items marked with a check " +
  "have been addressed in subsequent patches (1.0.7 ~ 1.0.9). Table of Contents Up next.";

function event(overrides: Partial<NotificationEvent> = {}): NotificationEvent {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    kind: "official_news",
    severity: "info",
    title: "Update Notes 1.0.9",
    body: ANUNCIO_LARGO,
    occurredAt: "2026-08-14T10:00:00Z",
    ...overrides,
  };
}

function inbox(items: NotificationEvent[]): NotificationInbox {
  return {
    items,
    total: items.length,
    limit: 40,
    offset: 0,
    unread: {
      total: items.length,
      info: items.length,
      success: 0,
      warning: 0,
      critical: 0,
    },
  };
}

function renderTray(onOpenGame?: (appId: number) => void) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <NotificationsPopover {...(onOpenGame ? { onOpenGame } : {})} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedApi.listNotificationRules.mockResolvedValue([]);
  mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
  mockedApi.epicFreeGames.mockResolvedValue([]);
  mockedApi.openEpicFreeGame.mockResolvedValue(undefined);
});

/**
 * La bandeja de avisos.
 *
 * Las publicaciones oficiales de Steam llegan con el anuncio entero. Tres de
 * ellas llenaban la bandeja y había que desplazarse para saber si quedaba algo
 * más: el aviso importante quedaba enterrado bajo el texto del anterior.
 */
describe("bandeja de avisos", () => {
  it("recorta un anuncio largo y deja leerlo entero bajo petición", async () => {
    const user = userEvent.setup();
    mockedApi.notificationInbox.mockResolvedValue(inbox([event()]));
    renderTray();

    await user.click(screen.getByRole("button", { name: /Avisos/ }));

    const fila = await screen.findByText("Update Notes 1.0.9");
    const texto = screen.getByText(ANUNCIO_LARGO);
    expect(fila).toBeVisible();
    // Recortado: el texto está entero en el documento, pero la fila no lo
    // enseña de golpe. Lo que se comprueba es la marca que lo recorta, porque
    // en jsdom no hay altura que medir.
    expect(texto).toHaveAttribute("data-clamped", "true");

    const mas = screen.getByRole("button", { name: "Leer más" });
    expect(mas).toHaveAttribute("aria-expanded", "false");
    await user.click(mas);
    expect(screen.getByText(ANUNCIO_LARGO)).toHaveAttribute("data-clamped", "false");

    await user.click(screen.getByRole("button", { name: "Leer menos" }));
    expect(screen.getByText(ANUNCIO_LARGO)).toHaveAttribute("data-clamped", "true");
  });

  /**
   * Un aviso cuenta algo que está en otra parte, y pulsarlo tiene que llevar
   * ahí. El de un regalo de Epic lleva a su ficha en la tienda, que es donde
   * se reclama.
   */
  it("un regalo de Epic lleva a su ficha en el navegador integrado", async () => {
    const user = userEvent.setup();
    mockedApi.notificationInbox.mockResolvedValue(
      inbox([
        event({
          id: "22222222-2222-4222-8222-222222222222",
          kind: "epic_free_game",
          title: "Edición estándar de Cardpocalypse",
          body: "Gratis en Epic hasta que acabe la promoción.",
          dedupeKey: "epic_free:of-1",
        }),
      ]),
    );
    mockedApi.epicFreeGames.mockResolvedValue([
      {
        offerId: "of-1",
        title: "Edición estándar de Cardpocalypse",
        description: "",
        storeUrl: "https://store.epicgames.com/es-ES/p/cardpocalypse",
        imageUrl: null,
        state: "current",
        startsAt: null,
        endsAt: null,
        originalPriceCents: 2399,
        currency: "EUR",
        owned: false,
        hoursLeft: 30,
        dismissed: false,
      },
    ]);
    renderTray();

    await user.click(screen.getByRole("button", { name: /Avisos/ }));
    const enlace = await screen.findByRole("button", {
      name: /Edición estándar de Cardpocalypse/,
    });
    await user.click(enlace);

    expect(mockedApi.openEpicFreeGame).toHaveBeenCalledWith(
      "https://store.epicgames.com/es-ES/p/cardpocalypse",
    );
    // Y se da por leído: se acaba de mirar.
    expect(mockedApi.markNotificationRead).toHaveBeenCalled();
  });

  it("un aviso sobre un juego lleva a su ficha", async () => {
    const user = userEvent.setup();
    const abrir = vi.fn();
    mockedApi.notificationInbox.mockResolvedValue(
      inbox([
        event({
          id: "33333333-3333-4333-8333-333333333333",
          appId: 620,
          gameTitle: "Portal 2",
          title: "Actualización publicada",
          body: "Hay novedades.",
        }),
      ]),
    );
    renderTray(abrir);

    await user.click(screen.getByRole("button", { name: /Avisos/ }));
    await user.click(await screen.findByRole("button", { name: /Actualización publicada/ }));

    expect(abrir).toHaveBeenCalledWith(620);
  });

  it("un aviso que no lleva a ninguna parte no finge que sí", async () => {
    mockedApi.notificationInbox.mockResolvedValue(
      inbox([event({ title: "Sin destino", body: "Nada que abrir." })]),
    );
    const user = userEvent.setup();
    renderTray();

    await user.click(screen.getByRole("button", { name: /Avisos/ }));
    expect(await screen.findByText("Sin destino")).toBeVisible();
    // El título es un párrafo, no un botón: no hay nada que pulsar.
    expect(screen.queryByRole("button", { name: /Sin destino/ })).toBeNull();
  });

  it("un aviso corto no ofrece desplegar lo que ya se ve entero", async () => {
    const user = userEvent.setup();
    mockedApi.notificationInbox.mockResolvedValue(
      inbox([event({ body: "El precio bajó de tu objetivo." })]),
    );
    renderTray();

    await user.click(screen.getByRole("button", { name: /Avisos/ }));
    await screen.findByText("El precio bajó de tu objetivo.");
    expect(screen.queryByRole("button", { name: "Leer más" })).toBeNull();
    expect(screen.getByText("El precio bajó de tu objetivo.")).toHaveAttribute(
      "data-clamped",
      "false",
    );
  });

  it("cada aviso se despliega por su cuenta", async () => {
    const user = userEvent.setup();
    mockedApi.notificationInbox.mockResolvedValue(
      inbox([
        event(),
        event({ id: "22222222-2222-4222-8222-222222222222", title: "Otro anuncio" }),
      ]),
    );
    renderTray();

    await user.click(screen.getByRole("button", { name: /Avisos/ }));
    const filas = await screen.findAllByRole("listitem");
    expect(filas).toHaveLength(2);

    await user.click(within(filas[0] as HTMLElement).getByRole("button", { name: "Leer más" }));
    expect(within(filas[0] as HTMLElement).getByText(ANUNCIO_LARGO)).toHaveAttribute(
      "data-clamped",
      "false",
    );
    // El de al lado sigue recortado: desplegar uno no despliega la bandeja.
    expect(within(filas[1] as HTMLElement).getByText(ANUNCIO_LARGO)).toHaveAttribute(
      "data-clamped",
      "true",
    );
  });
});
