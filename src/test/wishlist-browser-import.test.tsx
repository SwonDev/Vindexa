import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { importWishlistFromBrowser } from "@/features/wishlist/browser-import";
import { WishlistScreen } from "@/features/wishlist/WishlistScreen";
import { api } from "@/lib/tauri";
import type { WishlistImportReport, WishlistOverview } from "@/lib/types";

vi.mock("@/lib/tauri", () => ({
  api: {
    wishlistOverview: vi.fn(),
    wishlistPrices: vi.fn(),
    refreshWishlistPrices: vi.fn(),
    listCuratedLists: vi.fn(),
    importSteamWishlist: vi.fn(),
  },
  getErrorMessage: (error: unknown) => {
    if (error && typeof error === "object") {
      const candidate = error as { message?: unknown };
      if (typeof candidate.message === "string") return candidate.message;
    }
    return "Vindexa no pudo completar la operación.";
  },
}));

// Sólo se sustituye la llamada al backend: la decisión de cuándo ofrecer el
// camino del navegador es justo lo que estas pruebas comprueban, así que
// `suggestsBrowserImport` tiene que seguir siendo el de verdad.
vi.mock("@/features/wishlist/browser-import", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/features/wishlist/browser-import")>();
  return { ...original, importWishlistFromBrowser: vi.fn() };
});

const mockedApi = api as unknown as { [Key in keyof typeof api]: ReturnType<typeof vi.fn> };
const mockedBrowserImport = importWishlistFromBrowser as unknown as ReturnType<typeof vi.fn>;

const emptyOverview: WishlistOverview = {
  buckets: [],
  total: 0,
  targetTotals: [],
  entriesWithoutTarget: 0,
};

function report({
  fetched = 0,
  imported = 0,
  alreadyPresent = 0,
  limitReached = false,
}: {
  fetched?: number;
  imported?: number;
  alreadyPresent?: number;
  limitReached?: boolean;
} = {}): WishlistImportReport {
  return { fetched, imported, alreadyPresent, skipped: [], limitReached };
}

function renderScreen() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <WishlistScreen loading={false} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("deseados · importar desde la sesión del navegador", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedApi.wishlistOverview.mockResolvedValue(emptyOverview);
    mockedApi.wishlistPrices.mockResolvedValue([]);
    mockedApi.listCuratedLists.mockResolvedValue([]);
  });

  it("cuenta lo importado y no esconde los juegos que los filtros de Steam dejan fuera", async () => {
    mockedBrowserImport.mockResolvedValue({
      report: report({ fetched: 39, imported: 30, alreadyPresent: 9 }),
      steamId: "76561197960434622",
      titlesUnresolved: 0,
      hiddenByFilters: 2,
    });
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("button", { name: /desde el navegador/i }));

    const aviso = await screen.findByRole("status", { name: /importación/i });
    expect(aviso).toHaveTextContent("39 en Steam");
    expect(aviso).toHaveTextContent("30 nuevos");
    expect(aviso).toHaveTextContent("9 ya estaban");
    expect(aviso).toHaveTextContent("2 los esconden los filtros de tu lista en Steam");
  });

  it("ofrece el navegador justo cuando la API pública se queda sin lista por el perfil", async () => {
    mockedApi.importSteamWishlist.mockRejectedValue({
      code: "steam_wishlist_private",
      message: "Tu perfil de Steam no es público, así que Steam no deja leer la lista de deseados.",
    });
    mockedBrowserImport.mockResolvedValue({
      report: report({ fetched: 4, imported: 4 }),
      steamId: "76561197960434622",
      titlesUnresolved: 0,
      hiddenByFilters: 0,
    });
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("button", { name: /importar de steam/i }));

    const salida = await screen.findByRole("button", { name: /importar desde el navegador/i });
    await user.click(salida);

    await waitFor(() => expect(mockedBrowserImport).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole("status", { name: /importación/i })).toHaveTextContent(
      "4 nuevos",
    );
    // Resuelto el problema, la salida deja de ofrecerse.
    expect(
      screen.queryByRole("button", { name: /importar desde el navegador/i }),
    ).not.toBeInTheDocument();
  });

  it("no ofrece el navegador cuando el fallo es de otra cosa", async () => {
    mockedApi.importSteamWishlist.mockRejectedValue({
      code: "steam_wishlist_rate_limited",
      message: "Steam ha limitado temporalmente las peticiones.",
    });
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("button", { name: /importar de steam/i }));

    expect(await screen.findByRole("status", { name: /importación/i })).toHaveTextContent(
      "limitado temporalmente",
    );
    expect(
      screen.queryByRole("button", { name: /importar desde el navegador/i }),
    ).not.toBeInTheDocument();
  });

  it("repite tal cual lo que dice el backend cuando falta la sesión de Steam", async () => {
    mockedBrowserImport.mockRejectedValue({
      code: "wishlist_browser_signed_out",
      message:
        "Inicia sesión en Steam en la ventana que se ha abierto y vuelve a pulsar «Importar desde el navegador».",
    });
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("button", { name: /desde el navegador/i }));

    expect(await screen.findByRole("status", { name: /importación/i })).toHaveTextContent(
      "Inicia sesión en Steam en la ventana que se ha abierto",
    );
  });

  it("bloquea las dos importaciones mientras una está en marcha", async () => {
    let liberar: ((value: unknown) => void) | undefined;
    mockedBrowserImport.mockReturnValue(
      new Promise((resolve) => {
        liberar = resolve;
      }),
    );
    const user = userEvent.setup();
    renderScreen();

    await user.click(await screen.findByRole("button", { name: /desde el navegador/i }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /importar de steam/i })).toBeDisabled(),
    );
    expect(screen.getByRole("button", { name: /leyendo el navegador/i })).toBeDisabled();

    liberar?.({
      report: report({ fetched: 1, imported: 1 }),
      steamId: "76561197960434622",
      titlesUnresolved: 0,
      hiddenByFilters: 0,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /importar de steam/i })).toBeEnabled(),
    );
  });
});
