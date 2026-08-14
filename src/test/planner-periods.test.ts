import { describe, expect, it } from "vitest";
import {
  buildMonthSegments,
  buildPlannerMetrics,
  buildWeekSegments,
  type PlannerMetricColumn,
  type PlannerMetricSettings,
} from "@/features/planner/planner-periods";

const columns: PlannerMetricColumn[] = [
  {
    id: "playing",
    name: "Jugando ahora",
    wipLimit: 1,
    items: [
      {
        appId: 10,
        title: "Aventura",
        progress: 50,
        estimatedMinutes: 600,
        plannedFor: "2026-08-10",
      },
      {
        appId: 20,
        title: "Estrategia",
        progress: 0,
        estimatedMinutes: 300,
        plannedFor: "2026-08-16",
      },
    ],
  },
  {
    id: "later",
    name: "Más adelante",
    items: [
      {
        appId: 30,
        title: "Puzle",
        progress: 100,
        estimatedMinutes: 60,
      },
    ],
  },
];

const settings: PlannerMetricSettings = {
  weeklyCapacityMinutes: 720,
  monthlyCapacityMinutes: 1_200,
};

describe("periodos y capacidad del planificador", () => {
  it("calcula progreso, tiempo restante y sobrecarga sin contar dos veces un juego", () => {
    expect(buildPlannerMetrics(columns, settings, "2026-08-12")).toEqual({
      totalGames: 3,
      estimatedMinutes: 960,
      remainingMinutes: 600,
      averageProgress: 50,
      wipOverloadedColumns: 1,
      wipOverflowGames: 1,
      weekPlannedMinutes: 900,
      weekCapacityMinutes: 720,
      weekOverloadMinutes: 180,
      monthPlannedMinutes: 900,
      monthCapacityMinutes: 1_200,
      monthOverloadMinutes: 0,
      unscheduledGames: 1,
    });
  });

  it("segmenta una semana de lunes a domingo y conserva el día vacío", () => {
    const result = buildWeekSegments(columns, "2026-08-12");

    expect(result.rangeLabel).toBe("10–16 ago 2026");
    expect(result.segments).toHaveLength(7);
    expect(result.segments[0]).toMatchObject({ date: "2026-08-10", label: "Lun 10" });
    expect(result.segments[0].items.map((item) => item.appId)).toEqual([10]);
    expect(result.segments[2]).toMatchObject({ date: "2026-08-12", label: "Mié 12" });
    expect(result.segments[2].items).toEqual([]);
    expect(result.segments[6].items.map((item) => item.appId)).toEqual([20]);
    expect(result.unscheduled.map((item) => item.appId)).toEqual([30]);
  });

  it("agrupa el mes en semanas estables y deja fuera fechas de otro mes", () => {
    const result = buildMonthSegments(columns, "2026-08-12");

    expect(result.rangeLabel).toBe("Agosto 2026");
    expect(result.segments.map((segment) => segment.label)).toEqual([
      "27 jul–2 ago",
      "3–9 ago",
      "10–16 ago",
      "17–23 ago",
      "24–30 ago",
      "31 ago–6 sep",
    ]);
    expect(result.segments[2].items.map((item) => item.appId)).toEqual([10, 20]);
    expect(result.unscheduled.map((item) => item.appId)).toEqual([30]);
  });
});
