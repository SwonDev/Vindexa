import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationRulesPanel } from "@/features/notifications/NotificationRulesPanel";
import notificationsCss from "@/features/notifications/notifications.css?raw";
import {
  describeNextOccurrence,
  emptyRuleForm,
  formToInput,
  fromLocalInputValue,
  MAX_LEAD_MINUTES,
  pastDateWarning,
  releaseDateToLocalInput,
  toLocalInputValue,
  validateRuleForm,
} from "@/features/notifications/rule-form";
import { api } from "@/lib/tauri";
import type { NotificationRule } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    listNotificationRules: vi.fn(),
    saveNotificationRule: vi.fn(),
    deleteNotificationRule: vi.fn(),
    listGames: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const mockedApi = api as unknown as Record<string, ReturnType<typeof vi.fn>>;

/**
 * Regla mensual anclada al día 31. El backend conserva el día original: la cita
 * de febrero cae el 28 y la de marzo vuelve al 31. La interfaz tiene que pintar
 * la cita calculada, no el ancla.
 */
const monthlyRule: NotificationRule = {
  id: "11111111-1111-4111-8111-111111111111",
  appId: 730,
  gameTitle: "Juego Ancla",
  kind: "manual",
  title: "Repasar lo pendiente",
  body: "",
  scheduledFor: "2026-01-31T09:00:00Z",
  repeatRule: "monthly",
  leadMinutes: 60,
  enabled: true,
  lastFiredAt: "2026-01-31T08:00:00Z",
  currentOccurrence: "2026-01-31T09:00:00Z",
  nextOccurrence: "2026-02-28T09:00:00Z",
  createdAt: "2026-01-01T09:00:00Z",
  updatedAt: "2026-01-31T08:00:00Z",
};

const pausedRule: NotificationRule = {
  id: "22222222-2222-4222-8222-222222222222",
  kind: "manual",
  title: "Aviso en pausa",
  body: "",
  scheduledFor: "2026-03-01T18:00:00Z",
  repeatRule: "none",
  leadMinutes: 0,
  enabled: false,
  createdAt: "2026-01-01T09:00:00Z",
  updatedAt: "2026-01-01T09:00:00Z",
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <NotificationRulesPanel />
    </QueryClientProvider>,
  );
}

describe("validación del formulario de avisos", () => {
  it("exige un título y dice qué escribir", () => {
    const errors = validateRuleForm({ ...emptyRuleForm(), scheduledForLocal: "2026-09-01T18:30" });
    expect(errors.title).toBe("El aviso necesita un título: escribe qué quieres recordar.");
  });

  it("exige una fecha porque sin cita el aviso no se dispararía nunca", () => {
    const errors = validateRuleForm({ ...emptyRuleForm(), title: "Recordar" });
    expect(errors.scheduledForLocal).toMatch(/Sin fecha no hay cita/);
  });

  it("dice cuántos caracteres sobran en lugar de un «demasiado largo»", () => {
    const errors = validateRuleForm({
      ...emptyRuleForm(),
      title: "a".repeat(125),
      scheduledForLocal: "2026-09-01T18:30",
    });
    expect(errors.title).toBe("El título no puede superar 120 caracteres: sobran 5.");
  });

  it("respeta el techo de margen del backend", () => {
    const ok = validateRuleForm({
      ...emptyRuleForm(),
      title: "Recordar",
      scheduledForLocal: "2026-09-01T18:30",
      leadMinutes: MAX_LEAD_MINUTES,
    });
    expect(ok.leadMinutes).toBeUndefined();
    const tooMuch = validateRuleForm({
      ...emptyRuleForm(),
      title: "Recordar",
      scheduledForLocal: "2026-09-01T18:30",
      leadMinutes: MAX_LEAD_MINUTES + 1,
    });
    expect(tooMuch.leadMinutes).toBe(
      "El margen de aviso no puede superar 43200 minutos (30 días).",
    );
  });

  it("pide el juego solo en los tipos que hablan de un juego", () => {
    const base = { ...emptyRuleForm(), title: "Sale", scheduledForLocal: "2026-09-01T18:30" };
    expect(validateRuleForm(base).appId).toBeUndefined();
    expect(validateRuleForm({ ...base, kind: "release_date" }).appId).toMatch(
      /elige el juego antes de guardarlo/,
    );
    expect(validateRuleForm({ ...base, kind: "release_date", appId: 440 }).appId).toBeUndefined();
  });

  it("convierte la hora local en una marca con zona, que es lo único que acepta el backend", () => {
    const iso = fromLocalInputValue("2026-09-01T18:30");
    expect(iso).not.toBeNull();
    expect(iso).toMatch(/Z$/);
    // Ida y vuelta sin perder el minuto elegido.
    expect(toLocalInputValue(iso as string)).toBe("2026-09-01T18:30");
    expect(fromLocalInputValue("mañana por la tarde")).toBeNull();
  });

  it("no manda campos vacíos al backend", () => {
    const input = formToInput({
      ...emptyRuleForm(),
      title: "  Sale del acceso anticipado  ",
      scheduledForLocal: "2026-09-01T18:30",
    });
    expect(input.title).toBe("Sale del acceso anticipado");
    expect(input).not.toHaveProperty("id");
    expect(input).not.toHaveProperty("appId");
    expect(input).not.toHaveProperty("body");
    expect(input.repeatRule).toBe("none");
    expect(input.enabled).toBe(true);
  });

  it("avisa de una primera cita ya pasada sin bloquear el guardado", () => {
    const past = {
      ...emptyRuleForm(),
      title: "Recordar",
      scheduledForLocal: "2020-01-01T10:00",
    };
    expect(validateRuleForm(past).scheduledForLocal).toBeUndefined();
    expect(pastDateWarning(past)).toMatch(/ya pasó/);
    expect(pastDateWarning({ ...past, repeatRule: "monthly" })).toMatch(/cuenta como ancla/);
  });

  it("toma la fecha de un lanzamiento y deja la hora a quien programa", () => {
    expect(releaseDateToLocalInput("2026-11-04")).toBe("2026-11-04T09:00");
    expect(releaseDateToLocalInput("Q4 2026")).toBe("");
  });
});

describe("qué se pinta como próximo aviso", () => {
  it("usa la cita calculada y nunca el ancla", () => {
    const copy = describeNextOccurrence(monthlyRule);
    expect(copy.state).toBe("scheduled");
    // 28 de febrero: la cita real. El ancla (31 de enero) no puede aparecer.
    expect(copy.label).toContain("feb");
    expect(copy.label).not.toContain("ene");
  });

  it("distingue pausado de agotado", () => {
    expect(describeNextOccurrence(pausedRule).state).toBe("paused");
    expect(
      // Una regla agotada llega sin próxima fecha: la clave no viene.
      describeNextOccurrence(
        (({ nextOccurrence: _sinFecha, ...resto }) => ({
          ...resto,
          repeatRule: "none" as const,
        }))(monthlyRule),
      ).state,
    ).toBe("finished");
  });
});

describe("panel de avisos programados", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.listNotificationRules.mockResolvedValue([monthlyRule, pausedRule]);
    mockedApi.deleteNotificationRule.mockResolvedValue(undefined);
    mockedApi.listGames.mockResolvedValue({ items: [], total: 0, limit: 8, offset: 0 });
  });

  it("muestra la próxima cita calculada, no la primera que se eligió", async () => {
    renderPanel();

    expect(await screen.findByText("Repasar lo pendiente")).toBeVisible();
    expect(screen.getByText(/Próximo aviso: 28 feb 2026/)).toBeVisible();
    expect(screen.queryByText(/Próximo aviso: 31 ene 2026/)).toBeNull();
    expect(screen.getByText("Cada mes")).toBeVisible();
    expect(screen.getByText("1 hora antes")).toBeVisible();
  });

  it("pausa una regla conservando su ancla y su repetición", async () => {
    const user = userEvent.setup();
    mockedApi.saveNotificationRule.mockResolvedValue({ ...monthlyRule, enabled: false });
    renderPanel();

    await screen.findByText("Repasar lo pendiente");
    await user.click(screen.getByRole("switch", { name: "Pausar «Repasar lo pendiente»" }));

    await waitFor(() => expect(mockedApi.saveNotificationRule).toHaveBeenCalledTimes(1));
    expect(mockedApi.saveNotificationRule.mock.calls[0]?.[0]).toMatchObject({
      id: monthlyRule.id,
      // El ancla viaja intacta: reescribirla arrastraría el día del mes.
      scheduledFor: "2026-01-31T09:00:00Z",
      repeatRule: "monthly",
      leadMinutes: 60,
      enabled: false,
    });
  });

  it("borra solo tras una confirmación explícita", async () => {
    const user = userEvent.setup();
    renderPanel();

    await screen.findByText("Aviso en pausa");
    await user.click(screen.getByRole("button", { name: "Borrar «Aviso en pausa»" }));
    expect(mockedApi.deleteNotificationRule).not.toHaveBeenCalled();

    const confirm = screen.getByText(/¿Borrar «Aviso en pausa»\?/).closest("div");
    await user.click(within(confirm as HTMLElement).getByRole("button", { name: "Borrar" }));
    await waitFor(() =>
      expect(mockedApi.deleteNotificationRule).toHaveBeenCalledWith(pausedRule.id),
    );
  });

  it("filtra entre activos y pausados sin perder el recuento real", async () => {
    const user = userEvent.setup();
    renderPanel();

    await screen.findByText("Repasar lo pendiente");
    await user.click(screen.getByRole("button", { name: "Pausados" }));
    expect(screen.getByText("Aviso en pausa")).toBeVisible();
    expect(screen.queryByText("Repasar lo pendiente")).toBeNull();
    expect(screen.getByText(/de 2 activos/)).toBeVisible();
  });

  it("ofrece una pantalla de producto cuando no hay ningún aviso", async () => {
    mockedApi.listNotificationRules.mockResolvedValue([]);
    renderPanel();

    expect(await screen.findByText("Todavía no le has pedido nada a Vindexa")).toBeVisible();
    expect(screen.getByRole("button", { name: /Programar el primero/ })).toBeVisible();
  });

  it("crea un aviso desde el diálogo y solo con los datos válidos", async () => {
    const user = userEvent.setup();
    mockedApi.listNotificationRules.mockResolvedValue([]);
    mockedApi.saveNotificationRule.mockResolvedValue({
      ...monthlyRule,
      title: "Sale del acceso anticipado",
    });
    renderPanel();

    await user.click(await screen.findByRole("button", { name: /Programar el primero/ }));
    const dialog = await screen.findByRole("dialog");

    // Sin título ni fecha el envío no llega al backend y la interfaz dice qué
    // falta, no un «formulario inválido».
    await user.click(within(dialog).getByRole("button", { name: "Programar aviso" }));
    expect(mockedApi.saveNotificationRule).not.toHaveBeenCalled();
    expect(
      within(dialog).getByText("El aviso necesita un título: escribe qué quieres recordar."),
    ).toBeVisible();
    expect(within(dialog).getByText(/Sin fecha no hay cita/)).toBeVisible();

    await user.type(within(dialog).getByLabelText(/Título/), "Sale del acceso anticipado");
    fireEvent.change(within(dialog).getByLabelText(/Primera cita/), {
      target: { value: "2026-11-04T09:00" },
    });
    await user.click(within(dialog).getByRole("button", { name: "Programar aviso" }));

    await waitFor(() => expect(mockedApi.saveNotificationRule).toHaveBeenCalledTimes(1));
    const input = mockedApi.saveNotificationRule.mock.calls[0]?.[0];
    expect(input).toMatchObject({
      kind: "manual",
      title: "Sale del acceso anticipado",
      repeatRule: "none",
      enabled: true,
    });
    expect(input.scheduledFor).toBe(new Date("2026-11-04T09:00").toISOString());
  });
});

describe("contrato visual de la hoja de avisos", () => {
  it("mantiene la geometría técnica: ningún radio redondo ni mayor de 4 px", () => {
    const radii = notificationsCss.match(/border-radius:\s*([^;]+);/g) ?? [];
    expect(radii.length).toBeGreaterThan(0);
    for (const declaration of radii) {
      expect(declaration).not.toMatch(/9{3,}px|50%/);
      for (const value of declaration.match(/(\d+(?:\.\d+)?)px/g) ?? []) {
        expect(
          Number.parseFloat(value),
          `radio fuera de rango en «${declaration}»`,
        ).toBeLessThanOrEqual(4);
      }
    }
  });

  it("anima solo dentro de la ventana de 120–260 ms y con las curvas del sistema", () => {
    const durations = notificationsCss.match(/transition:[^;]+;/g) ?? [];
    expect(durations.length).toBeGreaterThan(0);
    for (const declaration of durations) {
      for (const value of declaration.match(/(\d+)ms/g) ?? []) {
        const ms = Number.parseInt(value, 10);
        expect(ms, `duración fuera de rango en «${declaration}»`).toBeGreaterThanOrEqual(120);
        expect(ms, `duración fuera de rango en «${declaration}»`).toBeLessThanOrEqual(260);
      }
      expect(declaration).not.toMatch(/cubic-bezier/);
    }
  });

  it("anula el movimiento con reducción de movimiento en lugar de atenuarlo", () => {
    expect(notificationsCss).toContain("@media (prefers-reduced-motion: reduce)");
    const reduced = notificationsCss.slice(notificationsCss.lastIndexOf("prefers-reduced-motion"));
    expect(reduced).toMatch(/transition:\s*none/);
  });
});
