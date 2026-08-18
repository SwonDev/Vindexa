import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StoresPanel } from "@/features/settings/StoresPanel";

/**
 * El bloque de itch.io del panel de tiendas.
 *
 * Se comprueba lo que la persona usuaria tiene derecho a saber: dónde acaba su
 * clave, qué se importa de su cuenta y —sobre todo— qué **no** se importa.
 * Ninguna prueba toca la red ni el llavero: `invoke` está simulado.
 */

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

vi.mock("@/lib/tauri", () => ({
  api: {
    detectExternalStores: vi.fn().mockResolvedValue([]),
    listExternalStoreAccounts: vi.fn().mockResolvedValue([]),
    listExternalGames: vi.fn().mockResolvedValue({ items: [], total: 0, limit: 60, offset: 0 }),
    scanExternalStore: vi.fn(),
    scanExternalStores: vi.fn(),
    setExternalGameMatch: vi.fn(),
    clearExternalGameMatch: vi.fn(),
    launchExternalGame: vi.fn(),
    linkExternalStore: vi.fn(),
    unlinkExternalStore: vi.fn(),
    rematchExternalStores: vi.fn(),
    listGames: vi.fn(),
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : "No se pudo completar la operación.",
}));

const SIN_CLAVE = {
  hasKey: false,
  account: null,
  lastImportAt: null,
  lastImportStatus: null,
  lastImportErrorMessage: null,
  gameCount: 0,
};

const CON_CLAVE = {
  hasKey: true,
  account: "Persona Usuaria",
  lastImportAt: "2026-08-17T10:00:00.000Z",
  lastImportStatus: "success" as const,
  lastImportErrorMessage: null,
  gameCount: 180,
};

const INFORME = {
  account: "Persona Usuaria",
  ownedKeys: 412,
  imported: 180,
  added: 25,
  alreadyPresent: 155,
  withoutCover: 4,
  skipped: 232,
  skippedGroups: [
    { reason: "assets", label: "Recursos gráficos o sonoros", count: 120 },
    { reason: "tool", label: "Herramientas y programas", count: 60 },
    { reason: "book", label: "Libros", count: 52 },
  ],
  matched: 12,
  truncated: false,
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <StoresPanel />
    </QueryClientProvider>,
  );
}

/** Devuelve el bloque de itch.io, para no confundirlo con el de Epic y GOG. */
function itchPanel(): HTMLElement {
  const heading = screen.getByRole("heading", { name: "itch.io" });
  const section = heading.closest("section");
  if (!section) {
    throw new Error("no se encontró la sección de itch.io");
  }
  return section;
}

describe("bloque de itch.io en Ajustes", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("dice dónde se genera la clave y dónde acaba guardada", async () => {
    invoke.mockResolvedValue(SIN_CLAVE);
    renderPanel();

    const panel = await waitFor(() => itchPanel());
    // Es un botón, no un enlace: dentro de la aplicación un `<a target="_blank">`
    // no abre nada, así que la dirección se entrega al navegador del sistema
    // por el mismo camino que la clave de Steam.
    const enlace = within(panel).getByRole("button", {
      name: "itch.io/user/settings/api-keys",
    });
    expect(enlace).toBeInTheDocument();

    expect(within(panel).getByText(/llavero de macOS/)).toBeInTheDocument();
    expect(within(panel).getByText(/itch-api-key/)).toBeInTheDocument();
    expect(within(panel).getByText(/No se escribe en la base de datos/)).toBeInTheDocument();
  });

  it("guarda la clave y la borra del formulario en cuanto deja de hacer falta", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "itch_session_state") {
        return Promise.resolve(SIN_CLAVE);
      }
      if (command === "save_itch_api_key") {
        return Promise.resolve({ username: "persona", displayName: "Persona Usuaria" });
      }
      return Promise.reject(new Error(`comando inesperado: ${command}`));
    });
    renderPanel();

    const panel = await waitFor(() => itchPanel());
    const campo = within(panel).getByLabelText("Clave de la API de itch.io");
    expect(campo).toHaveAttribute("type", "password");

    await userEvent.type(campo, "CLAVEDEPRUEBA123");
    await userEvent.click(within(panel).getByRole("button", { name: /Guardar clave y conectar/ }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("save_itch_api_key", { key: "CLAVEDEPRUEBA123" });
    });
    await waitFor(() => {
      expect(campo).toHaveValue("");
    });
    expect(
      await within(itchPanel()).findByText(/Sesión iniciada en itch.io como Persona Usuaria/),
    ).toBeInTheDocument();
  });

  it("una clave que itch.io rechaza se explica en español y no bloquea el panel", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "itch_session_state") {
        return Promise.resolve(SIN_CLAVE);
      }
      return Promise.reject(
        new Error(
          "itch.io no reconoce esta clave. Puede que la hayas revocado: genera una nueva y vuelve a pegarla.",
        ),
      );
    });
    renderPanel();

    const panel = await waitFor(() => itchPanel());
    await userEvent.type(
      within(panel).getByLabelText("Clave de la API de itch.io"),
      "CLAVEEQUIVOCADA1",
    );
    await userEvent.click(within(panel).getByRole("button", { name: /Guardar clave y conectar/ }));

    expect(
      await within(itchPanel()).findByText(/itch.io no reconoce esta clave/),
    ).toBeInTheDocument();
    // El formulario sigue disponible para volver a intentarlo.
    expect(within(itchPanel()).getByLabelText("Clave de la API de itch.io")).toBeInTheDocument();
  });

  it("con sesión iniciada muestra la cuenta y ofrece importar", async () => {
    invoke.mockResolvedValue(CON_CLAVE);
    renderPanel();

    expect(
      await within(itchPanel()).findByRole("button", { name: /Importar mi biblioteca/ }),
    ).toBeInTheDocument();
    const panel = itchPanel();
    expect(within(panel).getByText("Persona Usuaria")).toBeInTheDocument();
    expect(within(panel).getByText(/180 juegos de itch.io/)).toBeInTheDocument();
    // Con sesión iniciada ya no se pide la clave otra vez.
    expect(within(panel).queryByLabelText("Clave de la API de itch.io")).not.toBeInTheDocument();
  });

  it("el informe cuenta lo que entró y detalla lo que no es un juego", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "itch_session_state") {
        return Promise.resolve(CON_CLAVE);
      }
      if (command === "import_itch_library") {
        return Promise.resolve(INFORME);
      }
      return Promise.reject(new Error(`comando inesperado: ${command}`));
    });
    renderPanel();

    await userEvent.click(
      await within(itchPanel()).findByRole("button", { name: /Importar mi biblioteca/ }),
    );

    const informe = await within(itchPanel()).findByText(
      /Tu cuenta tiene 412 elementos\. 180 son juegos: 25 son nuevos y 155 ya estaban\./,
    );
    expect(informe).toBeInTheDocument();

    // Nada de «importado ✓»: lo que queda fuera se enumera.
    const resumen = within(itchPanel()).getByText(
      /232 elementos quedaron fuera porque no son juegos/,
    );
    expect(resumen).toBeInTheDocument();
    expect(within(itchPanel()).getByText("Recursos gráficos o sonoros: 120")).toBeInTheDocument();
    expect(within(itchPanel()).getByText("Herramientas y programas: 60")).toBeInTheDocument();
    expect(within(itchPanel()).getByText("Libros: 52")).toBeInTheDocument();
    expect(
      within(itchPanel()).getByText(/4 entraron sin carátula porque itch.io no publica ninguna/),
    ).toBeInTheDocument();
  });

  it("distingue cerrar sesión de borrar además lo importado", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "itch_session_state") {
        return Promise.resolve(CON_CLAVE);
      }
      return Promise.resolve(0);
    });
    renderPanel();

    await userEvent.click(
      await within(itchPanel()).findByRole("button", { name: /^Cerrar sesión$/ }),
    );
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("sign_out_itch");
    });
    expect(invoke).not.toHaveBeenCalledWith("forget_itch_library");

    await userEvent.click(
      within(itchPanel()).getByRole("button", { name: /Cerrar sesión y borrar lo importado/ }),
    );
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("forget_itch_library");
    });
  });

  it("una importación fallida se recuerda al volver a Ajustes", async () => {
    invoke.mockResolvedValue({
      ...CON_CLAVE,
      lastImportStatus: "failed",
      lastImportErrorMessage: "No se pudo conectar con itch.io. Comprueba tu conexión a internet.",
    });
    renderPanel();

    expect(
      await within(itchPanel()).findByText(
        /La última importación falló: No se pudo conectar con itch.io/,
      ),
    ).toBeInTheDocument();
  });
});
