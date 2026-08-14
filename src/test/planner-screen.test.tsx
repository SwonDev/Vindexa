import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
    listGames: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <span aria-hidden="true">{title.slice(0, 1)}</span>,
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
