import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PriorityExplanation } from "@/features/library/PriorityExplanation";
import { api } from "@/lib/tauri";
import type { PriorityExplanation as PriorityExplanationData } from "@/lib/types";
import "@/index.css";

vi.mock("@/lib/tauri", () => ({
  api: {
    explainPriority: vi.fn(),
    setPriorityLock: vi.fn(),
    recomputePriorities: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "Error inesperado",
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

/**
 * Suma comprobable: 24 + 20,5 + 3 − 30 − 5 = 12,5. Con `score` 52,5 el punto de
 * partida que deduce la interfaz es exactamente 40, la constante `BASE_SCORE`
 * del backend. Si el reparto dejase de cuadrar, la prueba lo cazaría.
 */
const derived: PriorityExplanationData = {
  appId: 620,
  title: "Portal 2",
  score: 52.5,
  effectiveScore: 52.5,
  derivedPriority: 3,
  manualPriority: 4,
  locked: false,
  reason: "Tienes una partida viva a medio camino y hace semanas que no la tocas.",
  computedAt: "2026-08-18T09:00:00Z",
  manualOverride: null,
  signals: [
    { signal: "progress_alive", weight: 24, detail: "Vas por el 60 % y esa partida sigue viva." },
    {
      signal: "completed_recently",
      weight: -30,
      detail: "Lo terminaste hace poco, así que deja sitio a lo que no has cerrado.",
    },
    {
      signal: "recent_sessions",
      weight: 20.5,
      detail: "Has abierto dos sesiones en las dos últimas semanas.",
    },
    { signal: "gone_cold", weight: -5, detail: "Llevas meses sin abrirlo." },
    { signal: "pinned", weight: 3, detail: "Lo tienes fijado en la biblioteca." },
  ],
};

const locked: PriorityExplanationData = {
  ...derived,
  effectiveScore: 100,
  derivedPriority: 2,
  manualPriority: 5,
  locked: true,
  manualOverride: "Tu prioridad manual dice 5; las señales dicen 2. Manda la tuya.",
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <PriorityExplanation appId={620} />
    </QueryClientProvider>,
  );
}

function signalRow(text: string): HTMLElement {
  const detail = screen.getByText(text);
  const row = detail.closest("li");
  if (!row) throw new Error(`No se encontró la señal «${text}».`);
  return row;
}

describe("prioridad explicable de la ficha", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.explainPriority.mockResolvedValue(derived);
    mockedApi.setPriorityLock.mockResolvedValue(undefined);
    mockedApi.recomputePriorities.mockResolvedValue({
      evaluated: 1,
      updated: 1,
      locked: 0,
      settled: 0,
      signalsWritten: 5,
      highlights: [],
      computedAt: "2026-08-18T10:00:00Z",
    });
  });

  it("encabeza la explicación con la frase del cálculo y la puntuación efectiva", async () => {
    const { container } = renderPanel();
    expect(
      await screen.findByText(
        "Tienes una partida viva a medio camino y hace semanas que no la tocas.",
      ),
    ).toBeVisible();
    expect(
      container.querySelector('.priority-score [data-slot="animated-number"]'),
    ).toHaveAttribute("data-value", "53");
    expect(screen.getByRole("heading", { name: "Por qué está aquí" })).toBeVisible();
    expect(mockedApi.explainPriority).toHaveBeenCalledWith(620);
  });

  it("desglosa cada señal con su aporte, positivo y negativo, sin depender del color", async () => {
    renderPanel();
    await screen.findByText("Vas por el 60 % y esa partida sigue viva.");

    const up = signalRow("Vas por el 60 % y esa partida sigue viva.");
    expect(up).toHaveAttribute("data-direction", "up");
    expect(up).toHaveAttribute("data-signal", "progress_alive");
    expect(within(up).getByText(/^\+24/)).toBeInTheDocument();
    expect(up).toHaveTextContent("sube la prioridad");

    const down = signalRow("Lo terminaste hace poco, así que deja sitio a lo que no has cerrado.");
    expect(down).toHaveAttribute("data-direction", "down");
    expect(within(down).getByText(/^-30/)).toBeInTheDocument();
    expect(down).toHaveTextContent("baja la prioridad");
  });

  it("deja la aritmética comprobable a ojo en lugar de pedir fe en el número", async () => {
    renderPanel();
    expect(
      await screen.findByText("Parte de 40 y las señales suman +12,5 → 52,5 sobre 100."),
    ).toBeVisible();
  });

  it("pliega las señales sobrantes con un control accesible", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Vas por el 60 % y esa partida sigue viva.");
    expect(screen.queryByText("Lo tienes fijado en la biblioteca.")).toBeNull();

    const toggle = screen.getByRole("button", { name: "Mostrar 1 señal más" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAttribute("aria-controls", "priority-signals-list");
    await user.click(toggle);
    expect(screen.getByText("Lo tienes fijado en la biblioteca.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Mostrar menos señales" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("ancla la prioridad manual y deja claro que a partir de ahí manda la persona", async () => {
    const user = userEvent.setup();
    mockedApi.explainPriority.mockResolvedValueOnce(derived).mockResolvedValue(locked);
    renderPanel();

    const anchor = await screen.findByRole("switch", { name: "Anclar mi prioridad manual" });
    expect(anchor).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getByText(
        "Actívalo y este juego se ordenará por la prioridad que fijes tú abajo, no por el cálculo.",
      ),
    ).toBeVisible();

    await user.click(anchor);
    await waitFor(() => expect(mockedApi.setPriorityLock).toHaveBeenCalledWith(620, true));
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Anclar mi prioridad manual" })).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
    expect(
      screen.getByText(
        "Este juego se ordena por tu 5/5. El cálculo sigue funcionando, pero ya no lo mueve.",
      ),
    ).toBeVisible();
  });

  it("mantiene una sola escala y dice en una frase quién manda cuando está anclada", async () => {
    mockedApi.explainPriority.mockResolvedValue(locked);
    const { container } = renderPanel();
    // La frase del backend es la que enfrenta las dos lecturas; ya no hacen
    // falta dos marcadores 0-5 al lado de un 0-100 que dice lo mismo.
    await screen.findByText("Tu prioridad manual dice 5; las señales dicen 2. Manda la tuya.");

    expect(container.querySelectorAll(".priority-verdict")).toHaveLength(0);
    expect(screen.queryByText("Tu prioridad")).toBeNull();
    expect(screen.queryByText("Las señales")).toBeNull();

    // La única cifra a la vista es la efectiva, y sigue siendo /100.
    expect(
      container.querySelector('.priority-score [data-slot="animated-number"]'),
    ).toHaveAttribute("data-value", "100");
    expect(
      within(container.querySelector(".priority-score") as HTMLElement).getByText("/100"),
    ).toBeVisible();
    // Anclar no borra el cálculo: la aritmética derivada sigue entera debajo.
    expect(
      screen.getByText("Parte de 40 y las señales suman +12,5 → 52,5 sobre 100."),
    ).toBeVisible();
  });

  it("permite recalcular cuando todavía no hay puntuación y avisa de su alcance", async () => {
    const user = userEvent.setup();
    mockedApi.explainPriority.mockResolvedValue({
      ...derived,
      computedAt: null,
      reason: "Todavía no se ha calculado la prioridad de este juego.",
      signals: [],
    });
    renderPanel();
    expect(await screen.findByText("Sin calcular todavía")).toBeVisible();
    expect(
      screen.getByText(
        "Todavía no hay señales guardadas para este juego. Se escriben al recalcular la prioridad de la biblioteca.",
      ),
    ).toBeVisible();

    const recompute = screen.getByRole("button", { name: "Recalcular la biblioteca" });
    // La frescura del cálculo dejó de ser una línea del cuerpo: viaja con el
    // único control que la usa, sin perderse para lectores de pantalla.
    expect(recompute).toHaveAttribute(
      "title",
      "Sin calcular todavía. Recalcula la prioridad de toda la biblioteca, no solo la de este juego.",
    );
    await user.click(recompute);
    await waitFor(() => expect(mockedApi.recomputePriorities).toHaveBeenCalledTimes(1));
  });

  it("informa del error exacto y permite reintentar sin inventar una puntuación", async () => {
    const user = userEvent.setup();
    mockedApi.explainPriority
      .mockRejectedValueOnce(new Error("El juego ya no está en la biblioteca."))
      .mockResolvedValue(derived);
    renderPanel();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("No se pudo explicar la prioridad de este juego.");
    expect(alert).toHaveTextContent("El juego ya no está en la biblioteca.");
    expect(document.querySelector(".priority-score")).toBeNull();

    await user.click(within(alert).getByRole("button", { name: "Reintentar" }));
    expect(
      await screen.findByText(
        "Tienes una partida viva a medio camino y hace semanas que no la tocas.",
      ),
    ).toBeVisible();
  });
});
