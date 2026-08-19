import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentChatPanel } from "@/features/agent/AgentChatPanel";
import { api } from "@/lib/tauri";

vi.mock("@/lib/tauri", async (original) => {
  const actual = await original<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      localModelSurvey: vi.fn(),
      vindagentConfig: vi.fn(),
      saveVindagentConfig: vi.fn(),
      vindagentChat: vi.fn(),
      speechEndpoint: vi.fn(async () => null),
      listAgentTasks: vi.fn(async () => []),
      saveAgentTask: vi.fn(),
      deleteAgentTask: vi.fn(async () => undefined),
    },
  };
});

const SIN_CONFIGURAR = {
  baseUrl: "",
  model: "",
  remoteAllowed: false,
  hasApiKey: false,
};

/** Una máquina con un modelo sirviendo en local. */
function conModelo(models = ["qwen-local"]) {
  return {
    runtimes: [],
    models: [],
    hardware: {
      totalMemoryBytes: null,
      cpuCores: null,
      architecture: "aarch64",
      usableModelBytes: null,
    },
    endpoints: [{ baseUrl: "http://127.0.0.1:8770", label: "llama.cpp", models }],
  };
}

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <AgentChatPanel onClose={vi.fn()} />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.vindagentConfig).mockResolvedValue(SIN_CONFIGURAR);
  vi.mocked(api.localModelSurvey).mockResolvedValue(conModelo());
});

describe("hablar con el agente de Vindexa", () => {
  it("sin modelo local lo dice y no deja escribir", async () => {
    // Fallar al enviar sería peor: aquí se explica qué falta antes de que
    // alguien escriba media frase para nada.
    vi.mocked(api.localModelSurvey).mockResolvedValue({ ...conModelo([]), endpoints: [] });
    renderPanel();

    expect(
      await screen.findByText(/No hay ningún modelo sirviendo en este ordenador/),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Mensaje para el agente")).toBeDisabled();
  });

  it("enseña la respuesta y lo que tocó por el camino", async () => {
    // Un agente que contesta «hecho» obliga a ir a comprobarlo. Los pasos son
    // la comprobación.
    const user = userEvent.setup();
    vi.mocked(api.vindagentChat).mockResolvedValue({
      reply: "Apuntadas 2 horas.",
      steps: [
        {
          tool: "registrar_sesion",
          arguments: { game: { name: "Hollow Knight" }, minutes: 120 },
          result: "{}",
          failed: false,
        },
      ],
    });
    renderPanel();

    // El campo existe antes de saber si hay modelo, y hasta entonces está
    // deshabilitado: escribir ahí no haría nada.
    const campo = await screen.findByLabelText("Mensaje para el agente");
    await waitFor(() => expect(campo).toBeEnabled());
    await user.type(campo, "he jugado dos horas a Hollow Knight");
    await user.click(screen.getByRole("button", { name: /Enviar/ }));

    expect(await screen.findByText("Apuntadas 2 horas.")).toBeInTheDocument();
    expect(screen.getByText("registrar_sesion")).toBeInTheDocument();
    // El paso resume sus argumentos en una línea: es lo que permite ver qué
    // tocó sin abrir nada. «Hollow Knight» sale dos veces —en lo que se
    // escribió y en el paso—, así que se busca el resumen del paso.
    expect(screen.getByText("game: Hollow Knight · minutes: 120")).toBeInTheDocument();
  });

  it("un fallo del modelo se cuenta, no se traga", async () => {
    const user = userEvent.setup();
    vi.mocked(api.vindagentChat).mockRejectedValue(new Error("El modelo tardó demasiado."));
    renderPanel();

    const campo = await screen.findByLabelText("Mensaje para el agente");
    await waitFor(() => expect(campo).toBeEnabled());
    await user.type(campo, "hola");
    await user.click(screen.getByRole("button", { name: /Enviar/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent("El modelo tardó demasiado.");
  });

  it("con un solo modelo no hay nada que elegir", async () => {
    renderPanel();
    await screen.findByLabelText("Mensaje para el agente");
    expect(screen.queryByLabelText("Modelo con el que hablar")).toBeNull();
  });

  it("con varios modelos se puede elegir, y la elección se guarda", async () => {
    const user = userEvent.setup();
    vi.mocked(api.localModelSurvey).mockResolvedValue(conModelo(["uno", "otro"]));
    vi.mocked(api.saveVindagentConfig).mockResolvedValue({
      ...SIN_CONFIGURAR,
      baseUrl: "http://127.0.0.1:8770",
      model: "otro",
    });
    renderPanel();

    const selector = await screen.findByLabelText("Modelo con el que hablar");
    await user.selectOptions(selector, "http://127.0.0.1:8770|otro");

    await waitFor(() =>
      expect(api.saveVindagentConfig).toHaveBeenCalledWith({
        baseUrl: "http://127.0.0.1:8770",
        model: "otro",
        remoteAllowed: false,
      }),
    );
  });

  it("sin transcriptor no aparece el botón de dictar", async () => {
    // Ofrecer algo que va a fallar es peor que no ofrecerlo.
    renderPanel();
    await screen.findByLabelText("Mensaje para el agente");
    expect(screen.queryByLabelText("Dictar")).toBeNull();
  });
});

describe("encargos del agente", () => {
  it("guarda un encargo con su cadencia", async () => {
    const user = userEvent.setup();
    vi.mocked(api.saveAgentTask).mockResolvedValue({
      id: "t1",
      instruction: "revisa el backlog",
      cadence: "semanal",
      enabled: true,
      lastRunAt: null,
      lastResult: null,
      createdAt: "2026-08-19T10:00:00Z",
    });
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Encargos" }));
    await user.type(screen.getByLabelText("Qué quieres que haga"), "revisa el backlog");
    await user.selectOptions(screen.getByLabelText("Cada cuánto"), "semanal");
    await user.click(screen.getByRole("button", { name: "Añadir" }));

    await waitFor(() =>
      expect(api.saveAgentTask).toHaveBeenCalledWith({
        instruction: "revisa el backlog",
        cadence: "semanal",
      }),
    );
  });

  it("enseña qué hizo la última vez, también cuando falló", async () => {
    // Un encargo que corre solo y no cuenta nada obliga a fiarse.
    vi.mocked(api.listAgentTasks).mockResolvedValue([
      {
        id: "t1",
        instruction: "sube a Backlog lo olvidado",
        cadence: "semanal",
        enabled: true,
        lastRunAt: new Date().toISOString(),
        lastResult: "Falló: el modelo no contestó",
        createdAt: "2026-08-01T10:00:00Z",
      },
    ]);
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Encargos" }));
    expect(await screen.findByText("sube a Backlog lo olvidado")).toBeInTheDocument();
    expect(screen.getByText("Falló: el modelo no contestó")).toBeInTheDocument();
  });

  it("sin modelo los encargos se guardan pero se avisa de que no correrán", async () => {
    vi.mocked(api.localModelSurvey).mockResolvedValue({ ...conModelo([]), endpoints: [] });
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Encargos" }));
    expect(screen.getByText(/se guardan pero no corren/)).toBeInTheDocument();
  });
});
