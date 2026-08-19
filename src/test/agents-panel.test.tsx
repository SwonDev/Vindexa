import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsPanel } from "@/features/settings/AgentsPanel";
import { api } from "@/lib/tauri";

vi.mock("@/lib/tauri", async (original) => {
  const actual = await original<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      listAgentClients: vi.fn(async () => []),
      listAgentAudit: vi.fn(async () => []),
      issueAgentClient: vi.fn(),
      rotateAgentToken: vi.fn(),
      setAgentClientEnabled: vi.fn(async () => undefined),
      revokeAgentClient: vi.fn(async () => undefined),
      agentUndo: vi.fn(),
      agentAutolinkState: vi.fn(async () => ({ disabled: false, links: [], hosts: [] })),
      setAgentAutolinkDisabled: vi.fn(async () => undefined),
      localModelSurvey: vi.fn(async () => ({
        runtimes: [],
        models: [],
        hardware: {
          totalMemoryBytes: null,
          cpuCores: null,
          architecture: "aarch64",
          usableModelBytes: null,
        },
      })),
    },
  };
});

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <AgentsPanel />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("agentes con acceso a Vindexa", () => {
  it("no da permisos de escritura por defecto", async () => {
    const user = userEvent.setup();
    vi.mocked(api.issueAgentClient).mockResolvedValue({
      client: {
        id: "c1",
        name: "Hermes",
        kind: "hermes",
        scopes: ["biblioteca:leer"],
        enabled: true,
        lastSeenAt: null,
        createdAt: "2026-08-19T10:00:00.000Z",
        updatedAt: "2026-08-19T10:00:00.000Z",
      },
      token: "vx_secreto",
    });
    renderPanel();

    await user.type(screen.getByLabelText("Nombre del agente"), "Hermes");
    await user.click(screen.getByRole("button", { name: /Emitir testigo/ }));

    // Un agente nace pudiendo mirar y nada más: los permisos se dan a mano.
    await waitFor(() =>
      expect(api.issueAgentClient).toHaveBeenCalledWith({
        name: "Hermes",
        kind: "hermes",
        scopes: ["biblioteca:leer"],
      }),
    );
  });

  it("enseña el testigo una sola vez y avisa de que no se recupera", async () => {
    const user = userEvent.setup();
    vi.mocked(api.issueAgentClient).mockResolvedValue({
      client: {
        id: "c1",
        name: "Hermes",
        kind: "hermes",
        scopes: ["biblioteca:leer", "sesiones:escribir"],
        enabled: true,
        lastSeenAt: null,
        createdAt: "2026-08-19T10:00:00.000Z",
        updatedAt: "2026-08-19T10:00:00.000Z",
      },
      token: "vx_testigo_secreto",
    });
    renderPanel();

    await user.type(screen.getByLabelText("Nombre del agente"), "Hermes");
    await user.click(screen.getByRole("switch", { name: /Registrar sesiones de juego/ }));
    await user.click(screen.getByRole("button", { name: /Emitir testigo/ }));

    expect(await screen.findByText("vx_testigo_secreto")).toBeVisible();
    expect(screen.getByText(/Vindexa guarda sólo su huella/)).toBeVisible();

    // Y desaparece en cuanto se confirma que está guardado: no se queda a la
    // vista de quien pase por delante de la pantalla.
    await user.click(screen.getByRole("button", { name: "Ya lo tengo guardado" }));
    expect(screen.queryByText("vx_testigo_secreto")).toBeNull();
  });

  it("enseña qué permite cada permiso, no cómo se llama por dentro", () => {
    renderPanel();

    expect(screen.getByText("Registrar sesiones de juego")).toBeVisible();
    expect(screen.getByText("Apuntar cuánto has jugado y por dónde vas.")).toBeVisible();
    // El identificador técnico no se enseña como si fuera la explicación.
    expect(screen.queryByText("sesiones:escribir")).toBeNull();
  });

  it("deja deshacer lo que un agente aplicó", async () => {
    const user = userEvent.setup();
    vi.mocked(api.listAgentAudit).mockResolvedValue([
      {
        id: "audit-1",
        clientId: "c1",
        clientName: "Hermes",
        intent: "registrar_sesion",
        utterance: "he jugado dos horas a DragonSword",
        arguments: {},
        result: "applied",
        affected: [{ appId: 620, title: "DragonSword: Awakening" }],
        undoable: true,
        errorMessage: null,
        createdAt: "2026-08-19T10:00:00.000Z",
        completedAt: "2026-08-19T10:00:01.000Z",
      },
    ]);
    vi.mocked(api.agentUndo).mockResolvedValue({ status: "undone" } as never);
    renderPanel();

    expect(await screen.findByText("registrar_sesion")).toBeVisible();
    expect(screen.getByText(/he jugado dos horas a DragonSword/)).toBeVisible();

    await user.click(screen.getByRole("button", { name: /Deshacer/ }));
    await waitFor(() => expect(api.agentUndo).toHaveBeenCalledWith("audit-1"));
  });
});

describe("lo que hay en este ordenador", () => {
  it("enseña los agentes encontrados y desde cuándo están conectados", async () => {
    vi.mocked(api.agentAutolinkState).mockResolvedValue({
      disabled: false,
      hosts: [
        {
          id: "hermes",
          label: "Hermes",
          path: "/Users/alguien/.local/bin/hermes",
          commandPreview: "hermes mcp add vindexa --args mcp",
        },
        { id: "claude", label: "Claude Code", path: null, commandPreview: "" },
      ],
      links: [
        {
          hostId: "hermes",
          clientId: "cli-1",
          command: "/Applications/Vindexa.app/Contents/MacOS/vindexa",
          linkedAt: new Date().toISOString(),
        },
      ],
    });
    renderPanel();

    expect(await screen.findByText("Hermes")).toBeInTheDocument();
    expect(screen.getByText(/Conectado/)).toBeInTheDocument();
    // Lo que no está instalado no se lista: ofrecer un agente que no existe
    // sólo sirve para que alguien lo busque.
    expect(screen.queryByText("Claude Code")).toBeNull();
  });

  it("sin memoria conocida no recomienda ningún tamaño", async () => {
    // Decir «te caben 8 GB» sin saber cuánta memoria hay es exactamente el
    // error que esta aplicación no comete.
    renderPanel();
    expect(
      await screen.findByText(/No se ha podido leer la memoria del sistema/),
    ).toBeInTheDocument();
  });

  it("marca un modelo que no le cabe a la máquina", async () => {
    vi.mocked(api.localModelSurvey).mockResolvedValue({
      runtimes: [
        {
          id: "llamacpp",
          label: "llama.cpp",
          path: "/opt/homebrew/bin/llama-server",
          formats: ["gguf"],
        },
      ],
      models: [
        {
          name: "Modelo-enorme",
          path: "/x/Modelo-enorme.gguf",
          format: "gguf",
          sizeBytes: 40 * 1024 ** 3,
          source: "Carpeta AI",
        },
      ],
      hardware: {
        totalMemoryBytes: 16 * 1024 ** 3,
        cpuCores: 8,
        architecture: "aarch64",
        usableModelBytes: 8 * 1024 ** 3,
      },
    });
    renderPanel();

    expect(await screen.findByText("llama.cpp")).toBeInTheDocument();
    expect(screen.getByText(/no le cabe a esta máquina/)).toBeInTheDocument();
  });
});
