import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { CollectionsScreen } from "@/features/collections/CollectionsScreen";
import { api } from "@/lib/tauri";
import type {
  AppBootstrap,
  CollectionSummary,
  GameSummary,
  PagedGames,
  SmartRule,
} from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    listGames: vi.fn(),
    listSmartRules: vi.fn(),
    saveCollection: vi.fn(),
    previewSmartCollection: vi.fn(),
    deleteCollection: vi.fn(),
    reorderCollections: vi.fn(),
    setGameCollections: vi.fn(),
    cacheGameArt: vi.fn(),
    updateGame: vi.fn(),
    openStore: vi.fn(),
    launchGame: vi.fn(),
    installGame: vi.fn(),
    revealInstallation: vi.fn(),
    setCollectionAppearance: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

const mockedApi = api as unknown as {
  [Key in keyof typeof api]: ReturnType<typeof vi.fn>;
};

function collection(overrides: Partial<CollectionSummary> & { id: string }): CollectionSummary {
  return {
    name: overrides.id,
    description: "",
    color: "#5CAAC1",
    icon: "folder",
    kind: "manual",
    matchMode: "all",
    position: 0,
    gameCount: 0,
    ...overrides,
  };
}

function game(appId: number, title: string, collectionIds: string[] = []): GameSummary {
  return {
    appId,
    title,
    coverUrl: `https://example.invalid/${appId}/cover.jpg`,
    playtimeMinutes: 120,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: true,
    statusId: "backlog",
    statusName: "Pendiente",
    statusColor: "#5CAAC1",
    progress: 42,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 0,
    collectionIds,
  };
}

function page(items: GameSummary[], total = items.length): PagedGames {
  return { items, total, limit: 60, offset: 0 };
}

const halfToldRules: SmartRule[] = [
  { id: "a", groupId: 0, field: "progress", operator: "greaterOrEqual", value: 20, position: 0 },
  { id: "b", groupId: 0, field: "progress", operator: "lessOrEqual", value: 80, position: 1 },
  { id: "c", groupId: 0, field: "statusId", operator: "notEquals", value: "done", position: 2 },
];

function makeBootstrap(collections: CollectionSummary[]): AppBootstrap {
  return {
    stats: {
      totalGames: 48,
      installedGames: 19,
      playingGames: 3,
      backlogGames: 10,
      trackedGames: 4,
      totalPlaytimeMinutes: 4200,
    },
    statuses: [
      {
        id: "backlog",
        name: "Pendiente",
        color: "#5CAAC1",
        position: 0,
        builtIn: true,
        gameCount: 10,
      },
      {
        id: "done",
        name: "Terminados",
        color: "#7EA64B",
        position: 1,
        builtIn: true,
        gameCount: 12,
      },
    ],
    collections,
    planner: [],
    steam: {
      apiKeyConfigured: false,
      apiKeyVerificationRequired: false,
      localSteamDetected: true,
      localManifestCount: 2,
    },
    preferences: {
      density: "compact",
      periodicSyncMinutes: 60,
      confirmUninstall: true,
      librarySort: "manual",
      shortcuts: {
        library: "Mod+1",
        planner: "Mod+2",
        collections: "Mod+3",
        tracking: "Mod+4",
        search: "Mod+K",
        sync: "Mod+Shift+S",
        closePanel: "Escape",
      },
    },
    databasePath: "/tmp/vindexa.sqlite3",
  };
}

const favourites = collection({
  id: "favourites",
  name: "Favoritos",
  description: "Los mundos a los que siempre merece la pena volver.",
  kind: "manual",
  gameCount: 3,
  position: 0,
});
const halfTold = collection({
  id: "half-told",
  name: "Historias a medias",
  description: "Campañas empezadas",
  color: "#A4D007",
  icon: "bookmark",
  kind: "smart",
  gameCount: 2,
  position: 1,
});

function renderScreen(bootstrap?: AppBootstrap, loading = false) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <CollectionsScreen bootstrap={bootstrap} loading={loading} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function mosaicCovers(name: string): NodeListOf<Element> {
  const tile = screen
    .getAllByRole("article")
    .find((candidate) => candidate.textContent?.includes(name));
  expect(tile).toBeDefined();
  return (tile as HTMLElement).querySelectorAll(
    '.collection-tile__mosaic img, .collection-tile__mosaic [role="img"]',
  );
}

describe("pantalla de colecciones", () => {
  beforeEach(() => {
    mockedApi.cacheGameArt.mockResolvedValue({
      appId: 1,
      variant: "cover",
      localPath: "/tmp/cover.jpg",
    });
    mockedApi.listSmartRules.mockResolvedValue(halfToldRules);
    mockedApi.deleteCollection.mockResolvedValue(undefined);
    mockedApi.reorderCollections.mockResolvedValue(undefined);
    mockedApi.setGameCollections.mockResolvedValue(undefined);
    mockedApi.listGames.mockImplementation(({ collectionId }: { collectionId?: string }) =>
      Promise.resolve(
        collectionId === "favourites"
          ? page([
              game(620, "Portal 2", ["favourites"]),
              game(1145360, "Hades", ["favourites", "half-told"]),
              game(367520, "Hollow Knight", ["favourites"]),
            ])
          : page([game(413150, "Stardew Valley"), game(105600, "Terraria")]),
      ),
    );
  });

  it("el clic derecho sobre una colección ofrece editarla, borrarla y cambiarle el aspecto", async () => {
    // Editar una colección desde aquí era ir a buscar el lápiz del pie de la
    // tarjeta. El gesto que todo el mundo prueba primero es el clic derecho.
    const user = userEvent.setup();
    renderScreen(makeBootstrap([favourites, halfTold]));

    const tarjeta = screen
      .getAllByRole("article")
      .find((tile) => tile.textContent?.includes("Favoritos"));
    expect(tarjeta).toBeDefined();
    await user.pointer({ keys: "[MouseRight]", target: tarjeta as HTMLElement });

    const menu = await screen.findByRole("menu", { name: /Acciones rápidas de Favoritos/ });
    expect(within(menu).getByRole("menuitem", { name: "Editar colección…" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Eliminar colección" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Color" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Icono" })).toBeVisible();
  });

  it("borrar desde el menú contextual pasa por la misma confirmación", async () => {
    // Un borrado que se dispara directo desde un menú es un borrado que ocurre
    // por accidente.
    const user = userEvent.setup();
    renderScreen(makeBootstrap([favourites, halfTold]));

    const tarjeta = screen
      .getAllByRole("article")
      .find((tile) => tile.textContent?.includes("Favoritos"));
    await user.pointer({ keys: "[MouseRight]", target: tarjeta as HTMLElement });
    await user.click(await screen.findByRole("menuitem", { name: "Eliminar colección" }));

    expect(await screen.findByRole("alertdialog")).toHaveTextContent(/¿Eliminar “Favoritos”\?/);
    expect(mockedApi.deleteCollection).not.toHaveBeenCalled();
  });

  it("el clic derecho sobre un juego de la colección ofrece sacarlo de ella", async () => {
    const user = userEvent.setup();
    renderScreen(makeBootstrap([favourites, halfTold]));

    const fila = await screen.findByRole("button", { name: /^Portal 2,/ });
    await user.pointer({ keys: "[MouseRight]", target: fila });

    const menu = await screen.findByRole("menu", { name: /Acciones rápidas de Portal 2/ });
    expect(within(menu).getByRole("menuitem", { name: "Abrir ficha" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Estado" })).toBeVisible();
    await user.click(within(menu).getByRole("menuitem", { name: /Quitar de Favoritos/ }));

    await waitFor(() =>
      expect(mockedApi.setGameCollections).toHaveBeenCalledWith(620, expect.any(Array)),
    );
  });

  it("una colección inteligente no ofrece sacar un juego a mano", async () => {
    // Sacarlo no serviría de nada: la regla lo devolvería en la siguiente
    // pasada. Ofrecerlo sería prometer algo que no se cumple.
    const user = userEvent.setup();
    renderScreen(makeBootstrap([halfTold, favourites]));

    const fila = await screen.findByRole("button", { name: /^Stardew Valley,/ });
    await user.pointer({ keys: "[MouseRight]", target: fila });

    const menu = await screen.findByRole("menu", { name: /Acciones rápidas de Stardew Valley/ });
    expect(within(menu).queryByRole("menuitem", { name: /Quitar de/ })).toBeNull();
  });

  it("pinta el icono elegido de cada colección, no uno genérico", async () => {
    // El icono se cambiaba y la vista principal seguía enseñando la carpeta de
    // siempre. Aquí se monta la pantalla de verdad y se mira lo que sale: que
    // el valor guardado llega hasta el SVG.
    renderScreen(
      makeBootstrap([
        collection({ id: "cohetes", name: "Para el espacio", icon: "rocket", position: 0 }),
        collection({ id: "raro", name: "Con un icono retirado", icon: "no-existe", position: 1 }),
      ]),
    );

    const espacio = screen
      .getAllByRole("article")
      .find((tile) => tile.textContent?.includes("Para el espacio"));
    expect(espacio?.querySelector(".tabler-icon-rocket")).not.toBeNull();

    // Un icono que ya no está en el catálogo no deja el hueco vacío: cae en la
    // carpeta, que es lo que significa «sin icono propio».
    const retirado = screen
      .getAllByRole("article")
      .find((tile) => tile.textContent?.includes("Con un icono retirado"));
    expect(retirado?.querySelector(".tabler-icon-folder")).not.toBeNull();
  });

  it("enseña las carátulas reales de cada colección sin pedir la biblioteca entera", async () => {
    renderScreen(makeBootstrap([favourites, halfTold]));

    await waitFor(() => expect(mosaicCovers("Favoritos")).toHaveLength(3));
    expect(mockedApi.listGames).toHaveBeenCalledWith({
      collectionId: "favourites",
      limit: 5,
      offset: 0,
    });
    // Los huecos restantes se reservan igualmente para que la geometría de la
    // tarjeta no dependa de cuántos juegos haya dentro.
    const favouritesTile = screen
      .getAllByRole("article")
      .find((tile) => tile.textContent?.includes("Favoritos")) as HTMLElement;
    expect(favouritesTile.querySelectorAll(".collection-tile__cover")).toHaveLength(5);
  });

  it("deja las reglas donde responden algo —la ficha— y abre la tarjeta con lo que significa", async () => {
    renderScreen(makeBootstrap([halfTold, favourites]));

    // La tarjeta enseña el resultado, no la maquinaria: primero la descripción
    // escrita por la persona; las reglas quedan a un `hover`.
    const smart = (await screen.findAllByRole("article")).find((tile) =>
      tile.textContent?.includes("Historias a medias"),
    ) as HTMLElement;
    const summary = smart.querySelector(".collection-tile__summary") as HTMLElement;
    expect(summary).toHaveTextContent("Campañas empezadas");
    await waitFor(() =>
      expect(summary).toHaveAttribute(
        "title",
        "Todas las reglas: Progreso entre 20 % y 80 % · Estado distinto de Terminados",
      ),
    );

    // En la ficha de la colección sí se leen enteras, y con el modo de
    // combinación delante: es la respuesta a «¿por qué está este juego dentro?».
    expect(
      await screen.findByText(
        "Todas las reglas: Progreso entre 20 % y 80 % · Estado distinto de Terminados",
      ),
    ).toBeVisible();

    await waitFor(() => expect(mockedApi.listSmartRules).toHaveBeenCalledWith("half-told"));
    // La colección manual no consulta reglas: no las tiene.
    expect(mockedApi.listSmartRules).not.toHaveBeenCalledWith("favourites");
  });

  it("distingue automática de manual con una marca propia, no solo con una palabra", async () => {
    renderScreen(makeBootstrap([favourites, halfTold]));

    const tiles = await screen.findAllByRole("article");
    const manual = tiles.find((tile) => tile.textContent?.includes("Favoritos")) as HTMLElement;
    const smart = tiles.find((tile) =>
      tile.textContent?.includes("Historias a medias"),
    ) as HTMLElement;
    expect(manual).toHaveAttribute("data-kind", "manual");
    expect(smart).toHaveAttribute("data-kind", "smart");
    expect(within(manual).getByText("MANUAL")).toBeInTheDocument();
    expect(within(smart).getByText("AUTOMÁTICA")).toBeInTheDocument();
  });

  it("una colección vacía no se parece a una llena y dice cómo llenarla", async () => {
    const emptyManual = collection({
      id: "empty-manual",
      name: "Pendientes de invierno",
      kind: "manual",
      gameCount: 0,
      position: 2,
    });
    renderScreen(makeBootstrap([favourites, halfTold, emptyManual]));

    const tiles = await screen.findAllByRole("article");
    const vacant = tiles.find((tile) =>
      tile.textContent?.includes("Pendientes de invierno"),
    ) as HTMLElement;
    expect(vacant).toHaveAttribute("data-empty", "true");
    expect(within(vacant).getByText("Vacía · arrastra juegos aquí")).toBeInTheDocument();
    // Y no gasta una petición en pedir el contenido de algo que sabe vacío.
    expect(mockedApi.listGames).not.toHaveBeenCalledWith(
      expect.objectContaining({ collectionId: "empty-manual" }),
    );

    const full = tiles.find((tile) => tile.textContent?.includes("Favoritos")) as HTMLElement;
    expect(full).toHaveAttribute("data-empty", "false");
  });

  it("muestra el contenido de la colección seleccionada en la misma pantalla", async () => {
    renderScreen(makeBootstrap([favourites, halfTold]));

    // Sin selección explícita se adopta la primera: la pantalla nunca queda vacía.
    const detail = await screen.findByRole("region", { name: "Contenido de Favoritos" });
    expect(
      await within(detail).findByRole("button", { name: /Portal 2, Pendiente/ }),
    ).toBeVisible();
    expect(within(detail).getByRole("button", { name: /Hollow Knight, Pendiente/ })).toBeVisible();

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Historias a medias" }));

    const smartDetail = await screen.findByRole("region", {
      name: "Contenido de Historias a medias",
    });
    expect(
      await within(smartDetail).findByRole("button", { name: /Stardew Valley/ }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: /Portal 2, Pendiente/ })).not.toBeInTheDocument();
  });

  it("saca un juego de una colección manual conservando el resto de sus colecciones", async () => {
    const user = userEvent.setup();
    renderScreen(makeBootstrap([favourites, halfTold]));

    await user.click(await screen.findByRole("button", { name: "Quitar Hades de Favoritos" }));

    await waitFor(() =>
      expect(mockedApi.setGameCollections).toHaveBeenCalledWith(1145360, ["half-told"]),
    );
  });

  it("no ofrece sacar juegos de una colección inteligente: la mantienen sus reglas", async () => {
    const user = userEvent.setup();
    renderScreen(makeBootstrap([halfTold, favourites]));

    expect(await screen.findByRole("button", { name: /Stardew Valley/ })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Quitar Stardew Valley de Historias a medias" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Favoritos" }));
    expect(
      await screen.findByRole("button", { name: "Quitar Portal 2 de Favoritos" }),
    ).toBeInTheDocument();
  });

  it("arrastra la tarjeta entera: no hay asa de seis puntos, solo un activador para teclado", async () => {
    renderScreen(makeBootstrap([favourites, halfTold]));

    const tiles = await screen.findAllByRole("article");
    expect(screen.queryByRole("button", { name: "Arrastrar Favoritos" })).not.toBeInTheDocument();
    const activator = screen.getByRole("button", { name: "Reordenar Favoritos" });
    expect(activator).toHaveClass("collection-drag-activator");
    // El gesto de puntero vive en la superficie completa de la tarjeta.
    expect(tiles[0]).toHaveClass("collection-tile");
    expect(tiles[0]?.getAttribute("data-kind")).toBe("manual");
  });

  it("propone plantillas accionables cuando todavía no hay ninguna colección", async () => {
    const user = userEvent.setup();
    renderScreen(makeBootstrap([]));

    expect(screen.getByRole("heading", { name: "Todavía no hay colecciones" })).toBeVisible();
    for (const name of ["Sesiones cortas", "Historias a medias", "Sin DRM"]) {
      expect(screen.getByRole("button", { name: new RegExp(name) })).toBeVisible();
    }

    await user.click(screen.getByRole("button", { name: /Historias a medias/ }));

    expect(await screen.findByRole("heading", { name: "Nueva colección" })).toBeVisible();
    expect(screen.getByLabelText("Nombre")).toHaveValue("Historias a medias");
    // La plantilla llega ya traducida a reglas verificables, listas para revisar.
    const fields = screen.getAllByRole("combobox", { name: "Campo de la regla" });
    expect(fields).toHaveLength(2);
    expect(fields[0]).toHaveTextContent("Progreso (%)");
  });

  it("reserva la geometría real de la tarjeta mientras carga, sin salto de maquetación", () => {
    const { container } = renderScreen(undefined, true);

    const skeletons = container.querySelectorAll(".collection-tile--skeleton");
    expect(skeletons).toHaveLength(6);
    expect(
      skeletons[0]?.querySelectorAll('[data-slot="shimmer-skeleton-block"]').length,
    ).toBeGreaterThanOrEqual(5);
    expect(screen.getByRole("status")).toHaveTextContent("Cargando colecciones");
  });

  // Debe ir en último lugar: retiene turnos del semáforo hasta liberarlos.
  it("limita las peticiones simultáneas de previsualización", async () => {
    const pending: ((value: PagedGames) => void)[] = [];
    let holding = true;
    mockedApi.listGames.mockImplementation(() =>
      holding
        ? new Promise<PagedGames>((resolve) => {
            pending.push(resolve);
          })
        : Promise.resolve(page([])),
    );

    const many = Array.from({ length: 12 }, (_, index) =>
      collection({
        id: `c${index}`,
        name: `Colección ${index}`,
        kind: "manual",
        gameCount: 4,
        position: index,
      }),
    );
    renderScreen(makeBootstrap(many));

    // Trece consultas quieren datos (doce tarjetas y el panel de detalle) pero
    // solo cuatro pueden estar en vuelo a la vez.
    await waitFor(() => expect(mockedApi.listGames.mock.calls.length).toBe(4));
    await Promise.resolve();
    expect(mockedApi.listGames.mock.calls.length).toBe(4);

    holding = false;
    for (const resolve of pending.splice(0)) resolve(page([]));
    await waitFor(() => expect(mockedApi.listGames.mock.calls.length).toBe(13));
  });
});
