import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  PlannerCapacityEditor,
  PlannerItemEditor,
  PlannerPeriodView,
  PlannerQueueView,
  type PlannerViewItem,
} from "@/features/planner/PlannerViews";
import { buildWeekSegments } from "@/features/planner/planner-periods";

vi.mock("@/components/common/Artwork", () => ({
  Artwork: ({ title }: { title: string }) => <span aria-hidden="true">{title.slice(0, 1)}</span>,
}));

const games: PlannerViewItem[] = [
  {
    appId: 10,
    title: "Aventura",
    progress: 50,
    position: 0,
    queuePosition: 0,
    estimatedMinutes: 600,
    plannedFor: "2026-08-10",
    targetDate: "2026-08-20",
    objective: "Completar el capítulo final",
  },
  {
    appId: 20,
    title: "Estrategia",
    progress: 0,
    position: 1,
    queuePosition: 1,
  },
];

describe("vistas avanzadas del planificador", () => {
  it("ofrece una cola lineal reordenable por teclado con límites inequívocos", async () => {
    const user = userEvent.setup();
    const onMove = vi.fn();
    render(
      <TooltipProvider>
        <PlannerQueueView items={games} onMove={onMove} onEdit={vi.fn()} />
      </TooltipProvider>,
    );

    expect(screen.getByRole("list", { name: "Cola lineal" })).toHaveTextContent(
      "Completar el capítulo final",
    );
    expect(screen.getByRole("button", { name: "Subir Aventura en la cola" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Bajar Estrategia en la cola" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Subir Estrategia en la cola" }));
    expect(onMove).toHaveBeenCalledWith(20, 0);
  });

  it("muestra los siete segmentos semanales y un vacío útil por día", () => {
    const period = buildWeekSegments(
      [{ id: "playing", name: "Jugando", items: games }],
      "2026-08-12",
    );
    render(<PlannerPeriodView period={period} label="Plan semanal" onEdit={vi.fn()} />);

    expect(screen.getByRole("region", { name: "Plan semanal" })).toHaveTextContent("Lun 10");
    expect(screen.getAllByText("Sin juegos programados")).toHaveLength(6);
    expect(screen.getByText("Aventura")).toBeInTheDocument();
  });

  it("guarda objetivo, planificación, fecha límite y estimación en una sola acción", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<PlannerItemEditor item={games[1]} onSave={onSave} />);

    await user.click(screen.getByRole("button", { name: "Planificar Estrategia" }));
    await user.type(screen.getByLabelText("Objetivo"), "Terminar la campaña");
    await user.type(screen.getByLabelText("Programado para"), "2026-08-14");
    await user.type(screen.getByLabelText("Fecha objetivo"), "2026-08-31");
    await user.clear(screen.getByLabelText("Horas estimadas"));
    await user.type(screen.getByLabelText("Horas estimadas"), "12.5");
    await user.click(screen.getByRole("button", { name: "Guardar planificación" }));

    expect(onSave).toHaveBeenCalledWith({
      appId: 20,
      objective: "Terminar la campaña",
      plannedFor: "2026-08-14",
      targetDate: "2026-08-31",
      estimatedMinutes: 750,
    });
  });

  it("permite ajustar la capacidad semanal y mensual sin aceptar un mes menor", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <PlannerCapacityEditor
        settings={{ weeklyCapacityMinutes: 600, monthlyCapacityMinutes: 2400 }}
        onSave={onSave}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Ajustar capacidad" }));
    await user.clear(screen.getByLabelText("Capacidad semanal en horas"));
    await user.type(screen.getByLabelText("Capacidad semanal en horas"), "12");
    await user.clear(screen.getByLabelText("Capacidad mensual en horas"));
    await user.type(screen.getByLabelText("Capacidad mensual en horas"), "8");
    await user.click(screen.getByRole("button", { name: "Guardar capacidad" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "La capacidad mensual debe ser igual o mayor que la semanal.",
    );
    expect(onSave).not.toHaveBeenCalled();

    await user.clear(screen.getByLabelText("Capacidad mensual en horas"));
    await user.type(screen.getByLabelText("Capacidad mensual en horas"), "48");
    await user.click(screen.getByRole("button", { name: "Guardar capacidad" }));
    expect(onSave).toHaveBeenCalledWith({
      weeklyCapacityMinutes: 720,
      monthlyCapacityMinutes: 2880,
    });
  });
});
