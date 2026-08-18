import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GameDlcPanel } from "@/features/library/GameDlcPanel";
import { api } from "@/lib/tauri";
import type { DlcRefreshReport, DlcSummary, GameDlc } from "@/lib/types";
import "@/index.css";

vi.mock("@/lib/tauri", () => ({
  api: {
    listGameDlc: vi.fn(),
    refreshGameDlc: vi.fn(),
    setDlcOwned: vi.fn(),
    setDlcHidden: vi.fn(),
    setDlcInstalled: vi.fn(),
    dlcSummary: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "Error inesperado",
}));

const mockedApi = api as unknown as Record<keyof typeof api, ReturnType<typeof vi.fn>>;

function dlc(overrides: Partial<GameDlc> & Pick<GameDlc, "dlcAppId" | "title">): GameDlc {
  return {
    appId: 620,
    isFree: false,
    owned: false,
    installed: false,
    hidden: false,
    metadataStatus: "success",
    position: 0,
    updatedAt: "2026-08-18T09:00:00Z",
    ...overrides,
  };
}

const items: GameDlc[] = [
  dlc({
    dlcAppId: 6201,
    title: "Banda sonora original",
    capsuleUrl: "https://example.test/capsule-6201.jpg",
    releaseDate: "2011-05-24",
    priceCents: 649,
    currency: "EUR",
    owned: true,
    installed: true,
    position: 0,
  }),
  dlc({
    dlcAppId: 6202,
    title: "Mapas de la comunidad",
    priceCents: 1299,
    currency: "EUR",
    position: 1,
  }),
  dlc({
    dlcAppId: 6203,
    title: "Sombrero de gala",
    metadataStatus: "unavailable",
    position: 2,
  }),
  dlc({
    dlcAppId: 6204,
    title: "Pack retirado",
    priceCents: 999,
    currency: "USD",
    position: 3,
  }),
  dlc({ dlcAppId: 6205, title: "Prueba cerrada", isFree: true, hidden: true, position: 4 }),
];

/** Con un desconocido y una divisa ajena, el importe no puede ser cerrado. */
const openSummary: DlcSummary = {
  appId: 620,
  total: 5,
  owned: 1,
  installed: 1,
  hidden: 1,
  free: 1,
  pending: 3,
  pendingValueCents: 1299,
  pendingValueCurrency: "EUR",
  pendingCounted: 1,
  pendingUnknownPrice: 1,
  pendingOtherCurrency: 1,
};

const closedSummary: DlcSummary = {
  appId: 620,
  total: 3,
  owned: 1,
  installed: 1,
  hidden: 0,
  free: 0,
  pending: 2,
  pendingValueCents: 1948,
  pendingValueCurrency: "EUR",
  pendingCounted: 2,
  pendingUnknownPrice: 0,
  pendingOtherCurrency: 0,
};

const report: DlcRefreshReport = {
  appId: 620,
  declared: 5,
  truncated: false,
  fetchedDetails: 4,
  unavailableDetails: 1,
  failedDetails: 0,
  pendingDetails: 0,
  ownershipEvidenceGap: "dlc_evidence_game_not_installed",
  ownershipEvidenceExplanation:
    "El juego no está instalado en este equipo, así que no hay manifiesto local que demuestre qué DLC posees.",
  imported: {
    appId: 620,
    received: 5,
    inserted: 5,
    updated: 0,
    withMetadata: 4,
    withoutMetadata: 1,
    owned: 1,
    installed: 1,
  },
  summary: openSummary,
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <GameDlcPanel appId={620} title="Portal 2" />
    </QueryClientProvider>,
  );
}

function rowOf(title: string): HTMLElement {
  const heading = screen.getByText(title);
  const row = heading.closest("li");
  if (!row) throw new Error(`No se encontró la fila del DLC «${title}».`);
  return row;
}

describe("gestión del contenido adicional", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.listGameDlc.mockResolvedValue(items);
    mockedApi.dlcSummary.mockResolvedValue(openSummary);
    mockedApi.refreshGameDlc.mockResolvedValue(report);
    mockedApi.setDlcOwned.mockImplementation(async (_appId, dlcAppId, owned) => ({
      ...dlc({ dlcAppId, title: "Mapas de la comunidad" }),
      owned,
    }));
    mockedApi.setDlcHidden.mockImplementation(async (_appId, dlcAppId, hidden) => ({
      ...dlc({ dlcAppId, title: "Mapas de la comunidad" }),
      hidden,
    }));
    mockedApi.setDlcInstalled.mockImplementation(async (_appId, dlcAppId, installed) => ({
      ...dlc({ dlcAppId, title: "Mapas de la comunidad" }),
      installed,
    }));
  });

  it("lista el contenido adicional con su precio, su estado y el recuento del juego", async () => {
    renderPanel();
    expect(await screen.findByText("Banda sonora original")).toBeVisible();
    expect(screen.getByText("Mapas de la comunidad")).toBeVisible();
    expect(screen.getByText("Sombrero de gala")).toBeVisible();
    expect(mockedApi.listGameDlc).toHaveBeenCalledWith(620, "visible");

    // Cada fila lleva su AppID: es el único identificador estable de un DLC.
    expect(within(rowOf("Banda sonora original")).getByText("APP 6201")).toBeVisible();
    expect(within(rowOf("Mapas de la comunidad")).getByText(/12,99/)).toBeVisible();
    // Sin precio publicado no se inventa un cero.
    expect(within(rowOf("Sombrero de gala")).getByText("Sin precio publicado")).toBeVisible();

    const declared = screen.getByText("Declarados").closest("div") as HTMLElement;
    expect(
      declared.querySelector('[data-slot="animated-number"]')?.getAttribute("data-value"),
    ).toBe("5");
  });

  it("marca «sin confirmar» lo que no tiene evidencia y jamás afirma que no lo tengas", async () => {
    renderPanel();
    await screen.findByText("Mapas de la comunidad");

    // Lo comprobado no lleva aviso; lo no comprobado sí, y con la palabra exacta.
    expect(within(rowOf("Banda sonora original")).queryByText("Sin confirmar")).toBeNull();
    const unconfirmed = within(rowOf("Mapas de la comunidad")).getByText("Sin confirmar");
    expect(unconfirmed).toHaveAttribute(
      "title",
      "Sin evidencia local de propiedad. No significa que no lo tengas.",
    );
    expect(screen.queryByText(/no lo tienes/i)).toBeNull();
    expect(screen.queryByText(/no poseído/i)).toBeNull();
    expect(screen.getByText(/significa que no hay evidencia, no que no lo tengas/i)).toBeVisible();
  });

  it("filtra por cada estado y llama al backend con el filtro exacto", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Mapas de la comunidad");

    await user.click(screen.getByRole("radio", { name: "Sin confirmar" }));
    await waitFor(() => expect(mockedApi.listGameDlc).toHaveBeenCalledWith(620, "notOwned"));
    await user.click(screen.getByRole("radio", { name: "Ocultos" }));
    await waitFor(() => expect(mockedApi.listGameDlc).toHaveBeenCalledWith(620, "hidden"));
    await user.click(screen.getByRole("radio", { name: "En propiedad" }));
    await waitFor(() => expect(mockedApi.listGameDlc).toHaveBeenCalledWith(620, "owned"));
    await user.click(screen.getByRole("radio", { name: "Todos" }));
    await waitFor(() => expect(mockedApi.listGameDlc).toHaveBeenCalledWith(620, "all"));
  });

  it("permite marcar propiedad, instalación y ocultación a mano", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Mapas de la comunidad");

    await user.click(screen.getByRole("button", { name: "En propiedad — Mapas de la comunidad" }));
    await waitFor(() => expect(mockedApi.setDlcOwned).toHaveBeenCalledWith(620, 6202, true));

    await user.click(screen.getByRole("button", { name: "Instalado — Mapas de la comunidad" }));
    await waitFor(() => expect(mockedApi.setDlcInstalled).toHaveBeenCalledWith(620, 6202, true));

    await user.click(screen.getByRole("button", { name: "Ocultar — Mapas de la comunidad" }));
    await waitFor(() => expect(mockedApi.setDlcHidden).toHaveBeenCalledWith(620, 6202, true));

    // Lo ya marcado se desmarca con el mismo control, sin menús escondidos.
    await user.click(screen.getByRole("button", { name: "En propiedad — Banda sonora original" }));
    await waitFor(() => expect(mockedApi.setDlcOwned).toHaveBeenCalledWith(620, 6201, false));
  });

  it("expone el estado marcado de cada control con aria-pressed", async () => {
    renderPanel();
    await screen.findByText("Banda sonora original");
    expect(
      screen.getByRole("button", { name: "En propiedad — Banda sonora original" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: "En propiedad — Mapas de la comunidad" }),
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("enseña el motivo exacto cuando no hay evidencia suficiente de propiedad", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Mapas de la comunidad");
    expect(screen.queryByText(/no hay manifiesto local/i)).toBeNull();

    await user.click(screen.getByRole("button", { name: "Actualizar desde la tienda" }));
    await waitFor(() => expect(mockedApi.refreshGameDlc).toHaveBeenCalledWith(620));

    const notice = await screen.findByText("El juego no está instalado en este equipo");
    const block = notice.closest(".dlc-evidence") as HTMLElement;
    expect(block).toHaveAttribute("data-gap", "dlc_evidence_game_not_installed");
    expect(within(block).getByText(report.ownershipEvidenceExplanation as string)).toBeVisible();
    // El código estable viaja con el aviso: es lo que permite reproducirlo.
    expect(within(block).getByText("dlc_evidence_game_not_installed")).toBeVisible();
    expect(within(block).getByText(/nunca como ausente/i, { exact: false })).toBeInTheDocument();
    expect(screen.getByText(/Steam declara 5 contenidos/)).toBeVisible();
  });

  it("presenta el importe pendiente como «al menos» y dice cuánto queda sin contar", async () => {
    renderPanel();
    await screen.findByText("Mapas de la comunidad");

    const label = screen.getByText("Pendiente: al menos");
    const block = label.closest(".dlc-pending") as HTMLElement;
    expect(block.querySelector(".dlc-pending__value")?.textContent).toMatch(/12,99/);
    expect(
      screen.getByText(
        "No es un total cerrado: suma 1 contenido con precio publicado y deja fuera 1 sin precio publicado y 1 en otra moneda.",
      ),
    ).toBeVisible();
  });

  it("cierra el importe sólo cuando no queda nada fuera de la suma", async () => {
    mockedApi.dlcSummary.mockResolvedValue(closedSummary);
    renderPanel();
    await screen.findByText("Mapas de la comunidad");

    expect(screen.queryByText("Pendiente: al menos")).toBeNull();
    expect(screen.getByText("Pendiente")).toBeVisible();
    expect(
      screen.getByText("Suma los 2 contenidos pendientes con precio publicado."),
    ).toBeVisible();
  });

  it("no inventa un importe cuando la tienda no publica ningún precio comparable", async () => {
    mockedApi.dlcSummary.mockResolvedValue({
      ...closedSummary,
      pending: 2,
      pendingValueCents: undefined,
      pendingValueCurrency: undefined,
      pendingCounted: 0,
      pendingUnknownPrice: 2,
    });
    renderPanel();
    await screen.findByText("Mapas de la comunidad");

    expect(screen.getByText("Sin importe que sumar")).toBeVisible();
    expect(
      screen.getByText(
        "Quedan 2 pendientes y la tienda no publica un precio comparable para ninguno: 2 sin precio publicado.",
      ),
    ).toBeVisible();
  });

  it("distingue la lista vacía por filtro de la ausencia de contenido adicional", async () => {
    mockedApi.listGameDlc.mockResolvedValue([]);
    renderPanel();
    expect(
      await screen.findByText("Ningún contenido adicional coincide con este filtro."),
    ).toBeVisible();
  });

  it("no afirma que un juego carezca de contenido antes de consultar la tienda", async () => {
    mockedApi.listGameDlc.mockResolvedValue([]);
    mockedApi.dlcSummary.mockResolvedValue({ ...openSummary, total: 0, pending: 0 });
    const user = userEvent.setup();
    renderPanel();
    expect(
      await screen.findByText(
        "Todavía no se ha consultado la tienda. Actualiza para saber si este juego tiene contenido adicional.",
      ),
    ).toBeVisible();

    mockedApi.refreshGameDlc.mockResolvedValue({
      ...report,
      declared: 0,
      fetchedDetails: 0,
      unavailableDetails: 0,
      ownershipEvidenceGap: undefined,
      ownershipEvidenceExplanation: undefined,
    });
    await user.click(screen.getByRole("button", { name: "Actualizar desde la tienda" }));
    expect(
      await screen.findByText("Steam no declara contenido adicional para este juego."),
    ).toBeVisible();
  });

  it("no pinta cientos de filas de golpe y deja llegar al resto con un control", async () => {
    const user = userEvent.setup();
    const many = Array.from({ length: 34 }, (_, index) =>
      dlc({ dlcAppId: 7000 + index, title: `Extra ${index + 1}`, position: index }),
    );
    mockedApi.listGameDlc.mockResolvedValue(many);
    renderPanel();
    await screen.findByText("Extra 1");
    expect(screen.getByText("Extra 30")).toBeVisible();
    expect(screen.queryByText("Extra 31")).toBeNull();

    const more = screen.getByRole("button", { name: "Mostrar 4 contenidos más" });
    expect(more).toHaveAttribute("aria-controls", "dlc-list");
    await user.click(more);
    expect(screen.getByText("Extra 34")).toBeVisible();
    expect(more).toHaveAttribute("aria-expanded", "true");
  });

  it("informa del fallo real sin vaciar la lista en silencio", async () => {
    mockedApi.refreshGameDlc.mockRejectedValueOnce(new Error("La tienda no respondió."));
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText("Mapas de la comunidad");
    await user.click(screen.getByRole("button", { name: "Actualizar desde la tienda" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No se pudo consultar la tienda: La tienda no respondió.",
    );
    expect(screen.getByText("Mapas de la comunidad")).toBeVisible();
  });
});
