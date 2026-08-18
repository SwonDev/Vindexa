import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  describeMatch,
  describeReleaseDate,
  UpcomingReleasesBlock,
} from "@/features/discovery/UpcomingReleasesBlock";
import { api } from "@/lib/tauri";
import type { TasteReport, UpcomingRelease } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    upcomingReleases: vi.fn(),
    dismissUpcomingRelease: vi.fn(),
    scoreUpcomingReleases: vi.fn(),
    recordTasteFeedback: vi.fn(),
    learnTaste: vi.fn(),
    listGames: vi.fn(),
    saveNotificationRule: vi.fn(),
    cacheGameArt: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

const exactRelease: UpcomingRelease = {
  appId: 9001,
  title: "Silksong del Norte",
  capsuleUrl: null,
  headerUrl: null,
  releaseDate: "2026-11-04",
  releaseDateIsExact: true,
  genres: ["Metroidvania"],
  categories: ["Un jugador"],
  developer: "Team Cherry",
  publisher: null,
  shortDescription: null,
  matchScore: 0.62,
  matchReason: "Coincide con tus 62 h en metroidvania y con Team Cherry.",
  source: "store",
  dismissedAt: null,
  discoveredAt: "2026-08-01T10:00:00Z",
  updatedAt: "2026-08-01T10:00:00Z",
};

const approximateRelease: UpcomingRelease = {
  ...exactRelease,
  appId: 9002,
  title: "Proyecto sin fecha",
  releaseDate: "Q4 2026",
  releaseDateIsExact: false,
  developer: "Estudio Lento",
  matchScore: 0.18,
  matchReason: "Coincide con tu interés por Un jugador.",
};

const report: TasteReport = {
  gamesAnalyzed: 412,
  dismissedUpcomingUsed: 3,
  facetsLearned: 57,
  positiveFacets: 41,
  negativeFacets: 16,
  highlights: [
    {
      facet: "genre",
      facetLabel: "Género",
      value: "Metroidvania",
      weight: 0.72,
      positiveSamples: 14,
      negativeSamples: 1,
    },
    {
      facet: "category",
      facetLabel: "Categoría",
      value: "Multijugador masivo",
      weight: -0.31,
      positiveSamples: 0,
      negativeSamples: 9,
    },
  ],
  computedAt: "2026-08-18T09:00:00Z",
};

/** El arte de reserva repite el título; se busca el titular real de la fila. */
const rowTitle = { selector: ".upcoming-row__title" } as const;

function renderBlock() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <UpcomingReleasesBlock />
    </QueryClientProvider>,
  );
}

describe("próximos lanzamientos puntuados", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.upcomingReleases.mockResolvedValue([exactRelease, approximateRelease]);
    mockedApi.recordTasteFeedback.mockResolvedValue(undefined);
    mockedApi.dismissUpcomingRelease.mockResolvedValue(undefined);
    mockedApi.learnTaste.mockResolvedValue(report);
    mockedApi.scoreUpcomingReleases.mockResolvedValue(2);
  });

  it("nunca muestra una puntuación sin la razón que la sostiene", async () => {
    renderBlock();

    expect(await screen.findByText("Silksong del Norte", rowTitle)).toBeVisible();
    expect(screen.getByText("Coincidencia alta · 62 %")).toBeVisible();
    expect(
      screen.getByText("Coincide con tus 62 h en metroidvania y con Team Cherry."),
    ).toBeVisible();
    expect(screen.getByText("Coincidencia baja · 18 %")).toBeVisible();
    expect(screen.getByText("Coincide con tu interés por Un jugador.")).toBeVisible();
  });

  it("distingue una fecha exacta de una aproximada también en el marcado", async () => {
    renderBlock();

    const exact = await screen.findByText("04 nov 2026");
    expect(exact.tagName).toBe("TIME");
    expect(exact).toHaveAttribute("datetime", "2026-11-04");
    expect(exact).toHaveAttribute("data-state", "exact");

    // La etiqueta aproximada se muestra tal cual la publica la tienda y no
    // finge ser una fecha legible por máquina.
    const approximate = screen.getByText("≈ Q4 2026");
    expect(approximate.tagName).not.toBe("TIME");
    expect(approximate).toHaveAttribute("data-state", "approximate");
  });

  it("dice en la interfaz que el modelo se calcula en el equipo", async () => {
    renderBlock();

    expect(
      await screen.findByText(/se calcula íntegramente en tu equipo/i, { exact: false }),
    ).toBeVisible();
  });

  it("registra la opinión sobre el candidato sin prometer un cambio inmediato", async () => {
    const user = userEvent.setup();
    renderBlock();

    const row = (await screen.findByText("Silksong del Norte", rowTitle)).closest("li");
    expect(row).not.toBeNull();
    await user.click(within(row as HTMLElement).getByRole("button", { name: /Me interesa/ }));

    await waitFor(() =>
      expect(mockedApi.recordTasteFeedback).toHaveBeenCalledWith(9001, "interested", "upcoming"),
    );
    expect(await screen.findByText(/Se aplicará en el próximo recálculo/i)).toBeVisible();
    expect(
      screen.getByText(/el modelo no cambia hasta que pulses/i, { exact: false }),
    ).toBeVisible();
  });

  it("descarta un candidato con el comando dedicado", async () => {
    const user = userEvent.setup();
    renderBlock();

    await screen.findByText("Proyecto sin fecha", rowTitle);
    await user.click(screen.getByRole("button", { name: "Descartar Proyecto sin fecha" }));

    await waitFor(() => expect(mockedApi.dismissUpcomingRelease).toHaveBeenCalledWith(9002));
  });

  it("recalcula aprendiendo primero y puntuando después, e informa de lo aprendido", async () => {
    const user = userEvent.setup();
    renderBlock();

    await screen.findByText("Silksong del Norte", rowTitle);
    await user.click(screen.getByRole("button", { name: /Recalcular/ }));

    await waitFor(() => expect(mockedApi.scoreUpcomingReleases).toHaveBeenCalledTimes(1));
    // Puntuar lee los pesos que acaba de escribir el aprendizaje: al revés se
    // puntuaría contra el modelo anterior.
    const learnedAt = mockedApi.learnTaste.mock.invocationCallOrder[0] as number;
    const scoredAt = mockedApi.scoreUpcomingReleases.mock.invocationCallOrder[0] as number;
    expect(learnedAt).toBeLessThan(scoredAt);

    expect(await screen.findByText("Qué aprendió el modelo")).toBeVisible();
    expect(screen.getByText("Juegos analizados")).toBeVisible();
    // `AnimatedNumber` duplica el texto en un doble oculto que reserva el ancho.
    expect(
      screen.getByText("412", { selector: "[data-slot='animated-number-value']" }),
    ).toBeVisible();
    expect(screen.getByText("Metroidvania")).toBeVisible();
    expect(screen.getByText("+0,72")).toBeVisible();
    expect(screen.getByText("-0,31")).toBeVisible();
  });

  it("mantiene un estado vacío honesto cuando no hay candidatos", async () => {
    mockedApi.upcomingReleases.mockResolvedValue([]);
    renderBlock();

    expect(await screen.findByText("Todavía no hay ningún candidato que puntuar")).toBeVisible();
    expect(screen.getByText(/no rellena esta lista con títulos inventados/i)).toBeVisible();
    // El control de aprendizaje sigue disponible: la biblioteca ya da señales.
    expect(screen.getByRole("button", { name: /Recalcular/ })).toBeEnabled();
  });
});

describe("traducción de la coincidencia y de la fecha", () => {
  it("acota la puntuación al rango real y nombra la banda", () => {
    expect(describeMatch(0)).toMatchObject({ percent: 0, level: "none" });
    expect(describeMatch(0.18)).toMatchObject({ percent: 18, level: "low" });
    expect(describeMatch(0.45)).toMatchObject({ percent: 45, level: "medium" });
    expect(describeMatch(0.9)).toMatchObject({ percent: 90, level: "high" });
    // Un valor imposible no se pinta como imposible.
    expect(describeMatch(4.2).percent).toBe(100);
    expect(describeMatch(Number.NaN).percent).toBe(0);
  });

  it("no reescribe una fecha aproximada como si fuera un día concreto", () => {
    expect(describeReleaseDate(approximateRelease)).toEqual({
      label: "Q4 2026",
      machine: null,
      state: "approximate",
    });
    expect(describeReleaseDate({ ...exactRelease, releaseDate: null })).toEqual({
      label: "Sin fecha anunciada",
      machine: null,
      state: "unknown",
    });
  });
});
