import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CouchScreen } from "@/features/couch/CouchScreen";
import { api } from "@/lib/tauri";
import type { GameSummary, PagedGames } from "@/lib/types";

const mocks = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    listGames: vi.fn(),
    gameDetail: vi.fn(),
    launchGame: vi.fn(),
    installGame: vi.fn(),
    openStore: vi.fn(),
    cacheGameArt: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: mocks.convertFileSrc,
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

function game(appId: number, title: string, installed = true): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 120,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed,
    statusId: "playing",
    statusName: "Jugando",
    statusColor: "#5CAAC1",
    progress: 40,
    priority: 2,
    pinned: false,
    tracking: false,
    manualPosition: appId,
    collectionIds: [],
    genres: [],
  };
}

const CATALOGUE = [
  game(1, "Alfa"),
  game(2, "Bravo"),
  game(3, "Charlie"),
  game(4, "Delta"),
  game(5, "Eco"),
  game(6, "Foxtrot", false),
];

function paged(items: GameSummary[]): PagedGames {
  return { items, total: items.length, limit: 240, offset: 0 };
}

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

function renderCouch(onExit = vi.fn()) {
  render(<CouchScreen onExit={onExit} />, { wrapper: Wrapper });
  return onExit;
}

beforeEach(() => {
  mockedApi.listGames.mockResolvedValue(paged(CATALOGUE));
  mockedApi.gameDetail.mockResolvedValue({
    ...CATALOGUE[0],
    shortDescription: "Una aventura corta.",
    developer: "Estudio Ejemplo",
    genres: [],
    categories: [],
    metadataStatus: "success",
    achievementsStatus: "success",
    collectionIds: [],
    tags: [],
    sessions: [],
    activity: [],
  });
  mockedApi.launchGame.mockResolvedValue(undefined);
  mockedApi.installGame.mockResolvedValue(undefined);
  mockedApi.openStore.mockResolvedValue(undefined);
});

describe("modo sofá", () => {
  it("enfoca el primer juego y enseña su ficha", async () => {
    renderCouch();
    const first = await screen.findByRole("button", { name: /^Alfa\./ });
    await waitFor(() => expect(first).toHaveFocus());
    expect(screen.getByRole("heading", { level: 2, name: "Alfa" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("Estudio Ejemplo")).toBeInTheDocument());
    expect(screen.getByText("Una aventura corta.")).toBeInTheDocument();
  });

  it("mueve el foco real del DOM con las flechas, en filas y columnas", async () => {
    const user = userEvent.setup();
    renderCouch();
    await waitFor(() => expect(screen.getByRole("button", { name: /^Alfa\./ })).toHaveFocus());

    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(screen.getByRole("button", { name: /^Bravo\./ })).toHaveFocus());

    // Cuatro columnas mientras no hay ancho medido: bajar salta una fila entera.
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(screen.getByRole("button", { name: /^Foxtrot\./ })).toHaveFocus());

    await user.keyboard("{ArrowUp}");
    await waitFor(() => expect(screen.getByRole("button", { name: /^Bravo\./ })).toHaveFocus());

    await user.keyboard("{Home}");
    await waitFor(() => expect(screen.getByRole("button", { name: /^Alfa\./ })).toHaveFocus());

    await user.keyboard("{End}");
    await waitFor(() => expect(screen.getByRole("button", { name: /^Foxtrot\./ })).toHaveFocus());
  });

  it("Intro lanza el juego enfocado y Escape sale del modo", async () => {
    const user = userEvent.setup();
    const onExit = renderCouch();
    await waitFor(() => expect(screen.getByRole("button", { name: /^Alfa\./ })).toHaveFocus());

    await user.keyboard("{Enter}");
    expect(mockedApi.launchGame).toHaveBeenCalledWith(1);
    expect(await screen.findByText("Steam recibió la solicitud para iniciar Alfa.")).toBeVisible();

    await user.keyboard("{Escape}");
    expect(onExit).toHaveBeenCalledTimes(1);
  });

  it("sobre un juego sin instalar la acción principal instala", async () => {
    const user = userEvent.setup();
    renderCouch();
    await waitFor(() => expect(screen.getByRole("button", { name: /^Alfa\./ })).toHaveFocus());
    await user.keyboard("{End}{Enter}");
    expect(mockedApi.installGame).toHaveBeenCalledWith(6);
    expect(mockedApi.launchGame).not.toHaveBeenCalled();
  });

  it("el ratón también recorre la rejilla y lanza desde la ficha", async () => {
    const user = userEvent.setup();
    renderCouch();
    await user.click(await screen.findByRole("button", { name: /^Charlie\./ }));
    expect(screen.getByRole("heading", { level: 2, name: "Charlie" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Jugar a Charlie" }));
    expect(mockedApi.launchGame).toHaveBeenCalledWith(3);
  });

  it("filtra a los instalados y vuelve a la biblioteca entera", async () => {
    const user = userEvent.setup();
    renderCouch();
    await screen.findByRole("button", { name: /^Alfa\./ });

    mockedApi.listGames.mockResolvedValue(paged(CATALOGUE.filter((entry) => entry.installed)));
    await user.click(screen.getByRole("button", { name: "Mostrar sólo los juegos instalados" }));
    await waitFor(() =>
      expect(mockedApi.listGames).toHaveBeenLastCalledWith({
        sort: "lastPlayed",
        limit: 240,
        installed: true,
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^Foxtrot\./ })).not.toBeInTheDocument(),
    );
  });

  it("informa cuando la biblioteca está vacía", async () => {
    mockedApi.listGames.mockResolvedValue(paged([]));
    renderCouch();
    expect(await screen.findByText("La biblioteca está vacía")).toBeInTheDocument();
  });

  it("informa del fallo y deja reintentar", async () => {
    mockedApi.listGames.mockRejectedValueOnce(new Error("SQLite no responde."));
    const user = userEvent.setup();
    renderCouch();
    expect(await screen.findByText("SQLite no responde.")).toBeInTheDocument();

    mockedApi.listGames.mockResolvedValue(paged(CATALOGUE));
    await user.click(screen.getByRole("button", { name: "Reintentar" }));
    expect(await screen.findByRole("button", { name: /^Alfa\./ })).toBeInTheDocument();
  });

  it("enseña la leyenda de botones y avisa de que no hay mando", async () => {
    renderCouch();
    await screen.findByRole("button", { name: /^Alfa\./ });
    const hints = screen.getByRole("list", { name: "Controles del modo sofá" });
    expect(hints).toHaveTextContent("Jugar");
    expect(hints).toHaveTextContent("Salir");
    expect(hints).toHaveTextContent("Mover el foco");
    // jsdom no expone la Gamepad API: el modo degrada a teclado y lo dice.
    expect(
      screen.getByText("Este entorno no expone la API de mandos: usa el teclado o el ratón."),
    ).toBeInTheDocument();
  });
});
