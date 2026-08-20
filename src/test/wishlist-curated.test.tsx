import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { CuratedListsPanel } from "@/features/wishlist/CuratedListsPanel";
import { api } from "@/lib/tauri";
import type { CuratedList, CuratedListDetail, CuratedListEntry, GameSummary } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    listCuratedLists: vi.fn(),
    saveCuratedList: vi.fn(),
    deleteCuratedList: vi.fn(),
    reorderCuratedLists: vi.fn(),
    curatedListDetail: vi.fn(),
    addCuratedGame: vi.fn(),
    updateCuratedItem: vi.fn(),
    removeCuratedGame: vi.fn(),
    moveCuratedItem: vi.fn(),
    reorderCuratedItems: vi.fn(),
    listGames: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

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
    drmState: "unknown",
    genres: [],
    collectionIds: [],
  };
}

function list(overrides: Partial<CuratedList> & { id: string; name: string }): CuratedList {
  return {
    description: "",
    kind: "manual",
    accent: "cyan",
    icon: "bookmark",
    pinned: false,
    position: 0,
    gameCount: 0,
    createdAt: "2026-08-01T10:00:00Z",
    updatedAt: "2026-08-01T10:00:00Z",
    ...overrides,
  };
}

function item(
  appId: number,
  title: string,
  position: number,
  extra: Partial<CuratedListEntry> = {},
): CuratedListEntry {
  return {
    game: game(appId, title),
    note: "",
    highlight: false,
    position,
    addedAt: "2026-08-01T10:00:00Z",
    ...extra,
  };
}

const entrada = list({
  id: "entrada-metroidvania",
  name: "Empezar en los metroidvania",
  description: "El orden en que se los enseñaría a alguien que nunca ha jugado a uno.",
  kind: "showcase",
  accent: "lime",
  gameCount: 2,
  position: 0,
  pinned: true,
});
const invierno = list({
  id: "invierno",
  name: "Para el invierno",
  kind: "backlog",
  accent: "amber",
  gameCount: 0,
  position: 1,
});

const entradaDetail: CuratedListDetail = {
  list: entrada,
  items: [
    item(367520, "Hollow Knight", 0, { note: "El punto de partida evidente.", highlight: true }),
    item(257850, "Hyper Light Drifter", 1),
  ],
  total: 2,
  limit: 60,
  offset: 0,
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <CuratedListsPanel />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("listas curadas", () => {
  beforeEach(() => {
    mockedApi.listCuratedLists.mockResolvedValue([entrada, invierno]);
    mockedApi.curatedListDetail.mockResolvedValue(entradaDetail);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
    mockedApi.saveCuratedList.mockImplementation((input: { name: string }) =>
      Promise.resolve(list({ id: "nueva", name: input.name })),
    );
    mockedApi.deleteCuratedList.mockResolvedValue(undefined);
    mockedApi.reorderCuratedLists.mockResolvedValue(undefined);
    mockedApi.addCuratedGame.mockResolvedValue(undefined);
    mockedApi.updateCuratedItem.mockResolvedValue(undefined);
    mockedApi.removeCuratedGame.mockResolvedValue(undefined);
    mockedApi.reorderCuratedItems.mockResolvedValue(undefined);
  });

  it("no se presenta como colecciones: enseña orden, nota y destacado", async () => {
    renderPanel();

    expect(
      await screen.findByRole("button", { name: "Empezar en los metroidvania" }),
    ).toBeVisible();
    const tile = screen
      .getAllByRole("article")
      .find((candidate) => candidate.textContent?.includes("Empezar en los metroidvania"));
    expect(tile).toBeDefined();
    // El tipo y el estado fijado se leen en la tarjeta, no solo en el detalle.
    expect(within(tile as HTMLElement).getByText("Vitrina")).toBeVisible();
    expect(within(tile as HTMLElement).getByText("FIJADA")).toBeVisible();
    expect(screen.getByText(/selecciones editoriales|Selecciones editoriales/)).toBeVisible();

    await screen.findByText("Hollow Knight");
    const detail = screen.getByRole("region", {
      name: "Contenido de Empezar en los metroidvania",
    });
    expect(within(detail).getByText("Hollow Knight")).toBeVisible();
    expect(within(detail).getByText("El punto de partida evidente.")).toBeVisible();
    // La segunda entrada no tiene nota: el hueco invita a escribirla.
    expect(within(detail).getByText("Añadir una nota")).toBeVisible();
    const rows = within(detail).getAllByRole("listitem");
    expect(rows[0]).toHaveAttribute("data-highlight", "true");
    expect(rows[1]).toHaveAttribute("data-highlight", "false");
  });

  it("destaca y deja de destacar una entrada sin perder su nota", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Hollow Knight");

    await user.click(screen.getByRole("button", { name: "Quitar destacado a Hollow Knight" }));

    await waitFor(() =>
      expect(mockedApi.updateCuratedItem).toHaveBeenCalledWith({
        listId: "entrada-metroidvania",
        appId: 367520,
        note: "El punto de partida evidente.",
        highlight: false,
      }),
    );
  });

  it("guarda la nota que se escribe sobre una entrada", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Hollow Knight");

    await user.click(screen.getByRole("button", { name: "Editar la nota de Hyper Light Drifter" }));
    await user.type(
      screen.getByRole("textbox", { name: "Nota de Hyper Light Drifter" }),
      "Para cuando el primero ya no sorprenda.",
    );
    await user.click(screen.getByRole("button", { name: /Guardar nota/ }));

    await waitFor(() =>
      expect(mockedApi.updateCuratedItem).toHaveBeenCalledWith({
        listId: "entrada-metroidvania",
        appId: 257850,
        note: "Para cuando el primero ya no sorprenda.",
        highlight: false,
      }),
    );
  });

  it("el orden manual es el contenido de la lista, así que se puede cambiar sin ratón", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Hollow Knight");

    await user.click(screen.getByRole("button", { name: "Bajar Hollow Knight" }));

    await waitFor(() =>
      expect(mockedApi.reorderCuratedItems).toHaveBeenCalledWith(
        "entrada-metroidvania",
        [257850, 367520],
      ),
    );
  });

  it("añade un juego desde la biblioteca y retira otro de la lista", async () => {
    const user = userEvent.setup();
    mockedApi.listGames.mockResolvedValue({
      items: [game(1145360, "Hades")],
      total: 1,
      limit: 8,
      offset: 0,
    });
    renderPanel();
    await screen.findByText("Hollow Knight");

    await user.type(screen.getByLabelText("Añadir a Empezar en los metroidvania"), "hades");
    await user.click(await screen.findByRole("button", { name: /Hades/ }));

    await waitFor(() =>
      expect(mockedApi.addCuratedGame).toHaveBeenCalledWith({
        listId: "entrada-metroidvania",
        appId: 1145360,
        note: "",
        highlight: false,
      }),
    );

    await user.click(
      screen.getByRole("button", {
        name: "Quitar Hollow Knight de Empezar en los metroidvania",
      }),
    );
    await waitFor(() =>
      expect(mockedApi.removeCuratedGame).toHaveBeenCalledWith("entrada-metroidvania", 367520),
    );
  });

  it("crea una lista con nombre, tipo y acento del sistema", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByRole("button", { name: "Empezar en los metroidvania" });

    await user.click(screen.getByRole("button", { name: /Nueva lista/ }));
    await user.type(screen.getByLabelText("Nombre"), "Cooperativos de sofá");
    await user.click(screen.getByRole("combobox", { name: "Tipo de lista curada" }));
    await user.click(await screen.findByRole("option", { name: "Pendientes" }));
    await user.click(screen.getByRole("combobox", { name: "Acento de la lista" }));
    await user.click(await screen.findByRole("option", { name: "Lima" }));
    await user.click(screen.getByRole("button", { name: /Guardar lista/ }));

    await waitFor(() =>
      expect(mockedApi.saveCuratedList).toHaveBeenCalledWith({
        name: "Cooperativos de sofá",
        description: "",
        kind: "backlog",
        accent: "lime",
        icon: "bookmark",
        pinned: false,
      }),
    );
  });

  it("no guarda una lista sin nombre", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByRole("button", { name: "Empezar en los metroidvania" });

    await user.click(screen.getByRole("button", { name: /Nueva lista/ }));
    await user.click(screen.getByRole("button", { name: /Guardar lista/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent("La lista necesita un nombre.");
    expect(mockedApi.saveCuratedList).not.toHaveBeenCalled();
  });

  it("reordena las listas con los controles de teclado, no solo arrastrando", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByRole("button", { name: "Empezar en los metroidvania" });

    await user.click(screen.getByRole("button", { name: "Bajar Empezar en los metroidvania" }));

    await waitFor(() =>
      expect(mockedApi.reorderCuratedLists).toHaveBeenCalledWith([
        "invierno",
        "entrada-metroidvania",
      ]),
    );
  });
});

describe("listas curadas · estados límite", () => {
  beforeEach(() => {
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
    mockedApi.curatedListDetail.mockResolvedValue(entradaDetail);
  });

  it("sin listas explica qué es una lista curada antes de pedir que se cree una", async () => {
    mockedApi.listCuratedLists.mockResolvedValue([]);
    renderPanel();

    expect(await screen.findByText("Todavía no hay ninguna lista curada")).toBeVisible();
    expect(screen.getByText(/nunca ha jugado a un metroidvania/)).toBeVisible();
    expect(screen.getByRole("button", { name: /Crear la primera/ })).toBeVisible();
  });

  it("una lista vacía dice que el orden es el argumento", async () => {
    mockedApi.listCuratedLists.mockResolvedValue([invierno]);
    mockedApi.curatedListDetail.mockResolvedValue({
      list: invierno,
      items: [],
      total: 0,
      limit: 60,
      offset: 0,
    });
    renderPanel();

    expect(await screen.findByText("La lista está vacía")).toBeVisible();
    expect(screen.getByText(/El orden en que los pongas es el argumento/)).toBeVisible();
  });

  it("cuando falla la lectura de las listas ofrece reintentar", async () => {
    mockedApi.listCuratedLists.mockRejectedValue(new Error("SQLite no responde."));
    renderPanel();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("No se pudieron leer las listas curadas");
    expect(within(alert).getByRole("button", { name: "Reintentar" })).toBeVisible();
  });

  it("cuando falla el contenido de una lista el resto de la pantalla sigue en pie", async () => {
    mockedApi.listCuratedLists.mockResolvedValue([entrada]);
    mockedApi.curatedListDetail.mockRejectedValue(new Error("Fila corrupta."));
    renderPanel();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("No se pudo leer el contenido");
    expect(screen.getByRole("button", { name: "Empezar en los metroidvania" })).toBeVisible();
  });
});

/**
 * El clic derecho en las listas curadas.
 *
 * Los botones de una baldosa y de cada juego sólo aparecen al pasar por encima
 * y son de dieciséis píxeles. El menú pone lo mismo donde se busca.
 */
describe("acciones rápidas de las listas curadas", () => {
  beforeEach(() => {
    mockedApi.listCuratedLists.mockResolvedValue([entrada, invierno]);
    mockedApi.curatedListDetail.mockResolvedValue(entradaDetail);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
    mockedApi.deleteCuratedList.mockResolvedValue(undefined);
    mockedApi.updateCuratedItem.mockResolvedValue(undefined);
    mockedApi.removeCuratedGame.mockResolvedValue(undefined);
  });

  it("una lista ofrece editarla, moverla y borrarla", async () => {
    const user = userEvent.setup();
    renderPanel();

    const baldosa = (await screen.findAllByRole("article")).find((nodo) =>
      nodo.textContent?.includes("Empezar en los metroidvania"),
    );
    expect(baldosa).toBeDefined();
    await user.pointer({ keys: "[MouseRight]", target: baldosa as HTMLElement });

    const menu = await screen.findByRole("menu", {
      name: /Acciones rápidas de Empezar en los metroidvania/,
    });
    expect(within(menu).getByRole("menuitem", { name: "Editar lista…" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Eliminar lista" })).toBeVisible();
  });

  it("borrar una lista desde el menú pasa por la confirmación", async () => {
    const user = userEvent.setup();
    renderPanel();

    const baldosa = (await screen.findAllByRole("article")).find((nodo) =>
      nodo.textContent?.includes("Empezar en los metroidvania"),
    );
    await user.pointer({ keys: "[MouseRight]", target: baldosa as HTMLElement });
    await user.click(await screen.findByRole("menuitem", { name: "Eliminar lista" }));

    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      /¿Eliminar «Empezar en los metroidvania»\?/,
    );
    expect(mockedApi.deleteCuratedList).not.toHaveBeenCalled();
  });

  it("un juego de la lista se destaca y se quita desde su menú", async () => {
    const user = userEvent.setup();
    renderPanel();

    const fila = (await screen.findByText("Hyper Light Drifter")).closest("li");
    expect(fila).not.toBeNull();
    await user.pointer({ keys: "[MouseRight]", target: fila as HTMLElement });

    const menu = await screen.findByRole("menu", {
      name: /Acciones rápidas de Hyper Light Drifter/,
    });
    expect(within(menu).getByRole("menuitem", { name: "Editar la nota" })).toBeVisible();
    await user.click(within(menu).getByRole("menuitem", { name: "Destacar" }));

    await waitFor(() =>
      expect(mockedApi.updateCuratedItem).toHaveBeenCalledWith(
        expect.objectContaining({ appId: 257850, highlight: true }),
      ),
    );
  });
});
