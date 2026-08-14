import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { CollectionsScreen } from "@/features/collections/CollectionsScreen";
import { LibraryScreen } from "@/features/library/LibraryScreen";
import { resetLibrarySessionForTests } from "@/features/library/library-session";
import { OrganizationSettings } from "@/features/settings/OrganizationSettings";
import { api } from "@/lib/tauri";
import type { AppBootstrap } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    saveStatus: vi.fn(),
    deleteStatus: vi.fn(),
    reorderStatuses: vi.fn(),
    savePlannerColumn: vi.fn(),
    deletePlannerColumn: vi.fn(),
    reorderPlannerColumns: vi.fn(),
    saveCollection: vi.fn(),
    previewSmartCollection: vi.fn(),
    listSmartRules: vi.fn(),
    deleteCollection: vi.fn(),
    reorderCollections: vi.fn(),
    libraryFilterOptions: vi.fn(),
    listGames: vi.fn(),
    listFamilyCatalog: vi.fn(),
    cacheGameArt: vi.fn(),
    startMetadataEnrichment: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        key: index,
        index,
        start: index * 320,
        size: 320,
        end: (index + 1) * 320,
        lane: 0,
      })),
    getTotalSize: () => count * 320,
    measure: vi.fn(),
  }),
}));

const mockedApi = api as unknown as {
  [Key in keyof typeof api]: ReturnType<typeof vi.fn>;
};

const bootstrap: AppBootstrap = {
  stats: {
    totalGames: 12,
    installedGames: 4,
    playingGames: 1,
    backlogGames: 6,
    trackedGames: 3,
    totalPlaytimeMinutes: 840,
  },
  statuses: [
    {
      id: "backlog",
      name: "Pendiente",
      color: "#5CAAC1",
      position: 0,
      builtIn: true,
      gameCount: 6,
    },
    {
      id: "paused",
      name: "En pausa",
      color: "#D6A64B",
      position: 1,
      builtIn: false,
      gameCount: 2,
    },
  ],
  collections: [
    {
      id: "cozy",
      name: "Noches tranquilas",
      description: "Partidas sin prisa",
      color: "#5CAAC1",
      icon: "sparkles",
      kind: "smart",
      matchMode: "all",
      position: 0,
      gameCount: 7,
    },
    {
      id: "coop",
      name: "Cooperativos",
      description: "Para jugar en grupo",
      color: "#A4D007",
      icon: "users",
      kind: "manual",
      matchMode: "all",
      position: 1,
      gameCount: 5,
    },
  ],
  planner: [
    {
      id: "this-week",
      name: "Esta semana",
      color: "#5CAAC1",
      position: 0,
      wipLimit: 5,
      items: [],
    },
    {
      id: "later",
      name: "Más adelante",
      color: "#788D9E",
      position: 1,
      items: [],
    },
  ],
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

function renderWithQuery(ui: React.ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>{ui}</TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("organización editable", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetLibrarySessionForTests();
    mockedApi.saveStatus.mockResolvedValue(undefined);
    mockedApi.reorderStatuses.mockResolvedValue(undefined);
    mockedApi.savePlannerColumn.mockResolvedValue(undefined);
    mockedApi.saveCollection.mockResolvedValue(undefined);
    mockedApi.listSmartRules.mockResolvedValue([
      {
        id: "rule-installed",
        groupId: 0,
        field: "installed",
        operator: "equals",
        value: true,
        position: 0,
      },
    ]);
    mockedApi.reorderCollections.mockResolvedValue(undefined);
    mockedApi.libraryFilterOptions.mockResolvedValue({
      genres: [],
      categories: [],
      tags: [],
      totalGames: 0,
      metadataGames: 0,
      achievementGames: 0,
      steamDeckGames: 0,
    });
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 240, offset: 0 });
    mockedApi.listFamilyCatalog.mockResolvedValue({
      items: [],
      total: 0,
      limit: 240,
      offset: 0,
    });
    mockedApi.cacheGameArt.mockResolvedValue({
      appId: 620,
      variant: "cover",
      localPath: "/tmp/portal-2.jpg",
    });
    mockedApi.startMetadataEnrichment.mockResolvedValue({
      running: false,
      totalGames: 1,
      freshMetadata: 1,
      queued: 0,
      processing: 0,
      retrying: 0,
      succeeded: 1,
      unavailable: 0,
      failed: 0,
      steamDeckAvailability: "disabled",
      steamDeckExplanation: "Steam no expone esta señal en la API pública utilizada.",
    });
  });

  it("renombra y recolorea un estado existente mediante el contrato persistente", async () => {
    const user = userEvent.setup();
    renderWithQuery(<OrganizationSettings bootstrap={bootstrap} />);

    await user.click(screen.getByRole("button", { name: "Editar Pendiente" }));
    const name = screen.getByRole("textbox", { name: "Nombre del estado Pendiente" });
    await user.clear(name);
    await user.type(name, "Por jugar");
    fireEvent.input(screen.getByLabelText("Color del estado Pendiente"), {
      target: { value: "#7ea64b" },
    });
    await user.click(screen.getByRole("button", { name: "Guardar cambios de Pendiente" }));

    await waitFor(() =>
      expect(mockedApi.saveStatus).toHaveBeenCalledWith("backlog", "Por jugar", "#7ea64b"),
    );
  });

  it("edita nombre, color y límite WIP de una columna existente", async () => {
    const user = userEvent.setup();
    renderWithQuery(<OrganizationSettings bootstrap={bootstrap} />);

    await user.click(screen.getByRole("button", { name: "Editar Esta semana" }));
    const name = screen.getByRole("textbox", { name: "Nombre de la columna Esta semana" });
    await user.clear(name);
    await user.type(name, "Próximos 7 días");
    fireEvent.input(screen.getByLabelText("Color de la columna Esta semana"), {
      target: { value: "#a4d007" },
    });
    const limit = screen.getByRole("spinbutton", { name: "Límite WIP de Esta semana" });
    await user.clear(limit);
    await user.type(limit, "0");
    expect(screen.getByRole("button", { name: "Guardar cambios de Esta semana" })).toBeDisabled();
    expect(mockedApi.savePlannerColumn).not.toHaveBeenCalled();
    await user.clear(limit);
    await user.type(limit, "4");
    await user.click(screen.getByRole("button", { name: "Guardar cambios de Esta semana" }));

    await waitFor(() =>
      expect(mockedApi.savePlannerColumn).toHaveBeenCalledWith(
        "this-week",
        "Próximos 7 días",
        "#a4d007",
        4,
      ),
    );
  });

  it("edita metadatos y reglas de una colección existente sin cambiar su tipo", async () => {
    const user = userEvent.setup();
    renderWithQuery(<CollectionsScreen bootstrap={bootstrap} loading={false} />);

    await user.click(screen.getByRole("button", { name: "Editar Noches tranquilas" }));
    expect(await screen.findByRole("heading", { name: "Editar colección" })).toBeVisible();
    expect(screen.getByDisplayValue("Partidas sin prisa")).toBeVisible();
    await waitFor(() => expect(mockedApi.listSmartRules).toHaveBeenCalledWith("cozy"));

    const name = screen.getByLabelText("Nombre");
    await user.clear(name);
    await user.type(name, "Noches acogedoras");
    fireEvent.input(screen.getByLabelText("Color de la colección"), {
      target: { value: "#d6a64b" },
    });
    await user.click(screen.getByRole("button", { name: "Guardar cambios" }));

    await waitFor(() =>
      expect(mockedApi.saveCollection).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "cozy",
          name: "Noches acogedoras",
          description: "Partidas sin prisa",
          color: "#d6a64b",
          icon: "sparkles",
          kind: "smart",
          rules: [expect.objectContaining({ id: "rule-installed", field: "installed" })],
        }),
      ),
    );
  });

  it("reordena las tarjetas con alternativa accesible y persiste el orden completo", async () => {
    const user = userEvent.setup();
    renderWithQuery(<CollectionsScreen bootstrap={bootstrap} loading={false} />);

    await user.click(screen.getByRole("button", { name: "Subir Cooperativos" }));

    await waitFor(() =>
      expect(mockedApi.reorderCollections).toHaveBeenCalledWith(["coop", "cozy"]),
    );
    const cards = screen.getAllByRole("article");
    expect(cards[0]).toHaveTextContent("Cooperativos");
    expect(cards[1]).toHaveTextContent("Noches tranquilas");
  });

  it("impide sobrescribir reglas cuando no puede cargar las guardadas y permite reintentar", async () => {
    const user = userEvent.setup();
    mockedApi.listSmartRules
      .mockRejectedValueOnce(new Error("No se pudieron leer las reglas."))
      .mockResolvedValueOnce([
        {
          id: "rule-installed",
          groupId: 0,
          field: "installed",
          operator: "equals",
          value: true,
          position: 0,
        },
      ]);
    renderWithQuery(<CollectionsScreen bootstrap={bootstrap} loading={false} />);

    await user.click(screen.getByRole("button", { name: "Editar Noches tranquilas" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudieron leer las reglas.");
    expect(screen.getByRole("button", { name: "Guardar cambios" })).toBeDisabled();
    expect(mockedApi.saveCollection).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Reintentar cargar reglas" }));
    await waitFor(() => expect(mockedApi.listSmartRules).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("button", { name: "Guardar cambios" })).toBeEnabled();
  });

  it("ofrece campos completos con operadores y valores tipados por el contrato", async () => {
    const user = userEvent.setup();
    renderWithQuery(<CollectionsScreen bootstrap={bootstrap} loading={false} />);

    await user.click(screen.getByRole("button", { name: "Nueva colección" }));
    await user.click(screen.getByRole("combobox", { name: "Campo de la regla" }));
    await user.click(await screen.findByRole("option", { name: "Fecha objetivo" }));

    const dateValue = screen.getByLabelText("Valor de la regla");
    expect(dateValue).toHaveAttribute("type", "date");
    await user.click(screen.getByRole("combobox", { name: "Operador de la regla" }));
    expect(screen.queryByRole("option", { name: "contiene" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "está definido" }));
    expect(screen.queryByLabelText("Valor de la regla")).not.toBeInTheDocument();
    expect(screen.getByText("Sin valor necesario")).toBeVisible();

    await user.click(screen.getByRole("combobox", { name: "Campo de la regla" }));
    await user.click(await screen.findByRole("option", { name: "Estado" }));
    await user.click(screen.getByRole("combobox", { name: "Estado de la regla" }));
    await user.click(await screen.findByRole("option", { name: "Pendiente" }));
    expect(screen.getByRole("combobox", { name: "Estado de la regla" })).toHaveTextContent(
      "Pendiente",
    );
    expect(screen.getByRole("combobox", { name: "Grupo de la regla" })).toHaveTextContent(
      "Grupo 1",
    );
  });

  it("conserva el catálogo de estados al crear desde la barra lateral de biblioteca", async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <LibraryScreen
        bootstrap={bootstrap}
        loading={false}
        error={undefined}
        onRetry={() => undefined}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Crear colección" }));
    expect(await screen.findByRole("heading", { name: "Nueva colección" })).toBeVisible();
    await user.click(screen.getByRole("combobox", { name: "Campo de la regla" }));
    await user.click(await screen.findByRole("option", { name: "Estado" }));
    await user.click(screen.getByRole("combobox", { name: "Estado de la regla" }));
    expect(await screen.findByRole("option", { name: "Pendiente" })).toBeVisible();
    expect(screen.getByRole("option", { name: "En pausa" })).toBeVisible();
  });

  it("descarta la selección al cambiar a Steam Family para no modificar juegos invisibles", async () => {
    const user = userEvent.setup();
    mockedApi.listGames.mockResolvedValue({
      items: [
        {
          appId: 620,
          title: "Portal 2",
          coverUrl: "https://shared.steamstatic.com/store_item_assets/steam/apps/620/cover.jpg",
          playtimeMinutes: 60,
          playtimeRecentMinutes: 0,
          isEarlyAccess: false,
          installed: true,
          statusId: "backlog",
          statusName: "Pendiente",
          statusColor: "#5CAAC1",
          progress: 0,
          priority: 0,
          pinned: false,
          tracking: false,
          manualPosition: 0,
        },
      ],
      total: 1,
      limit: 240,
      offset: 0,
    });
    renderWithQuery(
      <LibraryScreen
        bootstrap={bootstrap}
        loading={false}
        error={undefined}
        onRetry={() => undefined}
      />,
    );

    const game = await screen.findByRole("button", { name: /Portal 2, Pendiente/ });
    await user.click(game, { ctrlKey: true });
    expect(screen.getByText("1 seleccionado")).toBeVisible();

    await user.click(screen.getByRole("button", { name: /Steam Family/ }));
    expect(screen.queryByText("1 seleccionado")).not.toBeInTheDocument();
  });
});
