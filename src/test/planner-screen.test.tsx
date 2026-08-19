import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { PlannerScreen } from "@/features/planner/PlannerScreen";
import { api } from "@/lib/tauri";
import type { AppBootstrap } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    getPlannerOverview: vi.fn(),
    movePlannerItem: vi.fn(),
    movePlannerQueueItem: vi.fn(),
    savePlannerItem: vi.fn(),
    savePlannerCapacity: vi.fn(),
    savePlannerColumn: vi.fn(),
    removePlannerItem: vi.fn(),
    listGames: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <span aria-hidden="true">{title.slice(0, 1)}</span>,
  // La precarga es una mejora de tiempos: en pruebas basta con que exista.
  prefetchArtwork: () => undefined,
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;
const columns = [
  {
    id: "playing",
    name: "Jugando ahora",
    color: "#5CAAC1",
    position: 0,
    wipLimit: 3,
    items: [
      {
        appId: 10,
        title: "Aventura",
        progress: 50,
        position: 0,
        queuePosition: 0,
        estimatedMinutes: 600,
        plannedFor: "2026-08-10",
        objective: "Llegar al epílogo",
      },
      {
        appId: 20,
        title: "Estrategia",
        progress: 10,
        position: 1,
        queuePosition: 1,
      },
    ],
  },
];
const overview = {
  columns,
  queue: columns[0]?.items ?? [],
  settings: { weeklyCapacityMinutes: 720, monthlyCapacityMinutes: 2400 },
};
const bootstrap = { planner: columns } as unknown as AppBootstrap;

function renderPlanner(initialBootstrap: AppBootstrap | null = bootstrap) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <PlannerScreen
          {...(initialBootstrap ? { bootstrap: initialBootstrap } : {})}
          loading={false}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("planificador completo", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.getPlannerOverview.mockResolvedValue(overview);
    mockedApi.movePlannerQueueItem.mockResolvedValue(undefined);
    mockedApi.savePlannerItem.mockResolvedValue(undefined);
    mockedApi.savePlannerCapacity.mockResolvedValue(undefined);
  });

  it("cambia entre kanban, cola, semana y mes sin perder los juegos persistidos", async () => {
    const user = userEvent.setup();
    renderPlanner();

    expect(await screen.findByRole("tab", { name: "Kanban" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Cola" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Semana" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Mes" })).toBeVisible();

    await user.click(screen.getByRole("tab", { name: "Cola" }));
    expect(screen.getByRole("list", { name: "Cola lineal" })).toHaveTextContent("Aventura");
    await user.click(screen.getByRole("button", { name: "Subir Estrategia en la cola" }));
    expect(mockedApi.movePlannerQueueItem).toHaveBeenCalledWith(20, 0);

    await user.click(screen.getByRole("tab", { name: "Semana" }));
    expect(screen.getByRole("region", { name: "Plan semanal" })).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "Mes" }));
    expect(screen.getByRole("region", { name: "Plan mensual" })).toBeVisible();
  });

  it("expone el error de SQLite y permite reintentar", async () => {
    const user = userEvent.setup();
    mockedApi.getPlannerOverview.mockRejectedValue(new Error("SQLite no pudo leer el plan."));
    renderPlanner(null);

    expect(await screen.findByRole("alert")).toHaveTextContent("SQLite no pudo leer el plan.");
    mockedApi.getPlannerOverview.mockResolvedValue(overview);
    await user.click(screen.getByRole("button", { name: "Reintentar planificador" }));
    await waitFor(() => expect(mockedApi.getPlannerOverview).toHaveBeenCalledTimes(2));
  });
});

/**
 * El clic derecho en el planificador.
 *
 * Aquí vive la única forma de sacar un juego del plan: `remove_planner_item`
 * llevaba escrito desde el principio y ninguna pantalla lo llamaba. Y el color
 * y el límite de una columna estaban a tres clics, en Ajustes, mientras se
 * planifica mirando la columna.
 */
/**
 * Los submenús se recorren con el teclado.
 *
 * jsdom devuelve rectángulos vacíos, así que el «polígono de gracia» con el que
 * Radix mantiene abierto un submenú al mover el ratón no se puede calcular y el
 * puntero lo cerraría siempre. Es una limitación del entorno de pruebas, no de
 * la aplicación.
 */
/** Baja por el submenú hasta el elemento pedido y lo activa. */
async function elegirEnSubmenu(
  user: ReturnType<typeof userEvent.setup>,
  rol: "menuitem" | "menuitemradio",
  nombre: string,
) {
  const destino = screen.getByRole(rol, { name: nombre });
  for (let paso = 0; paso < 20 && document.activeElement !== destino; paso += 1) {
    await user.keyboard("{ArrowDown}");
  }
  await waitFor(() => expect(destino).toHaveFocus());
  await user.keyboard("{Enter}");
}

async function abrirSubmenu(
  user: ReturnType<typeof userEvent.setup>,
  nombre: string,
  posicion: number,
) {
  const disparador = screen.getByRole("menuitem", { name: nombre });
  for (let paso = 0; paso < posicion; paso += 1) await user.keyboard("{ArrowDown}");
  await waitFor(() => expect(disparador).toHaveFocus());
  await user.keyboard("{ArrowRight}");
  return screen.findByRole("menu", { name: nombre });
}

describe("acciones rápidas del planificador", () => {
  beforeEach(() => {
    mockedApi.getPlannerOverview.mockResolvedValue(overview);
    mockedApi.removePlannerItem.mockResolvedValue(undefined);
    mockedApi.savePlannerColumn.mockResolvedValue(undefined);
    mockedApi.movePlannerItem.mockResolvedValue(undefined);
  });

  it("una tarjeta ofrece ficha, planificación, mover y salir del plan", async () => {
    const user = userEvent.setup();
    renderPlanner();

    const tarjeta = await screen.findByText("Aventura");
    await user.pointer({ keys: "[MouseRight]", target: tarjeta });

    const menu = await screen.findByRole("menu", { name: /Acciones rápidas de Aventura/ });
    expect(within(menu).getByRole("menuitem", { name: "Abrir ficha" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Editar planificación…" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Quitar del planificador" })).toBeVisible();
  });

  it("sacar del plan pregunta antes, porque se pierde lo escrito a mano", async () => {
    const user = userEvent.setup();
    renderPlanner();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("Aventura") });
    await user.click(await screen.findByRole("menuitem", { name: "Quitar del planificador" }));

    const dialogo = await screen.findByRole("alertdialog");
    expect(dialogo).toHaveTextContent(/objetivo, la fecha y la estimación/);
    expect(mockedApi.removePlannerItem).not.toHaveBeenCalled();

    await user.click(within(dialogo).getByRole("button", { name: "Quitar del plan" }));
    await waitFor(() => expect(mockedApi.removePlannerItem).toHaveBeenCalledWith(10));
  });

  it("una columna cambia de color desde su cabecera", async () => {
    const user = userEvent.setup();
    renderPlanner();

    const cabecera = await screen.findByRole("heading", { name: "Jugando ahora" });
    await user.pointer({ keys: "[MouseRight]", target: cabecera });
    await screen.findByRole("menu", { name: /Acciones rápidas de Jugando ahora/ });

    await abrirSubmenu(user, "Color", 1);
    await elegirEnSubmenu(user, "menuitem", "Lima");

    // El nombre y el límite viajan intactos: cambiar el color no puede
    // llevarse por delante el resto de la columna.
    await waitFor(() =>
      expect(mockedApi.savePlannerColumn).toHaveBeenCalledWith(
        "playing",
        "Jugando ahora",
        "#A4D007",
        3,
      ),
    );
  });

  it("una columna cambia su límite de trabajo sin ir a Ajustes", async () => {
    const user = userEvent.setup();
    renderPlanner();

    await user.pointer({
      keys: "[MouseRight]",
      target: await screen.findByRole("heading", { name: "Jugando ahora" }),
    });
    await screen.findByRole("menu", { name: /Acciones rápidas de Jugando ahora/ });

    await abrirSubmenu(user, "Límite de trabajo", 2);
    await elegirEnSubmenu(user, "menuitemradio", "5 juegos");

    await waitFor(() =>
      expect(mockedApi.savePlannerColumn).toHaveBeenCalledWith(
        "playing",
        "Jugando ahora",
        "#5CAAC1",
        5,
      ),
    );
  });
});
