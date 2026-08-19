import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { prefetchArtwork } from "@/components/common/Artwork";
import { useIdleArtworkPrefetch } from "@/features/library/use-idle-artwork-prefetch";
import { api } from "@/lib/tauri";

vi.mock("@/components/common/Artwork", () => ({ prefetchArtwork: vi.fn() }));

vi.mock("@/lib/tauri", async (original) => {
  const actual = await original<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    api: { ...actual.api, listArtworkTargets: vi.fn(), getArtCacheUsage: vi.fn() },
  };
});

/** Ejecuta los huecos de reposo de inmediato, para no esperar al navegador. */
function conReposoInmediato() {
  const pendientes: (() => void)[] = [];
  Object.defineProperty(window, "requestIdleCallback", {
    value: (callback: () => void) => {
      pendientes.push(callback);
      return pendientes.length;
    },
    configurable: true,
  });
  Object.defineProperty(window, "cancelIdleCallback", {
    value: () => undefined,
    configurable: true,
  });
  return {
    async agotar(vueltas = 10) {
      for (let vuelta = 0; vuelta < vueltas; vuelta += 1) {
        const siguiente = pendientes.shift();
        if (!siguiente) return;
        siguiente();
        await Promise.resolve();
      }
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  // Por defecto, disco de sobra: cada prueba que quiera lo contrario lo dice.
  vi.mocked(api.getArtCacheUsage).mockResolvedValue({
    bytes: 10_000_000,
    budgetBytes: 512 * 1024 * 1024,
  });
});

afterEach(() => {
  Reflect.deleteProperty(window, "requestIdleCallback");
  Reflect.deleteProperty(window, "cancelIdleCallback");
});

describe("completar la caché de arte en los ratos libres", () => {
  it("recorre la biblioteca entera por tandas", async () => {
    const reposo = conReposoInmediato();
    const objetivos = Array.from({ length: 50 }, (_, index) => ({
      appId: 1000 + index,
      coverUrl: `https://shared.steamstatic.com/store_item_assets/steam/apps/${1000 + index}/library_600x900_2x.jpg`,
    }));
    vi.mocked(api.listArtworkTargets).mockResolvedValue(objetivos);

    renderHook(() => useIdleArtworkPrefetch(true));

    await waitFor(() => expect(api.listArtworkTargets).toHaveBeenCalledTimes(1));
    await reposo.agotar();

    // Se reparte en tandas en vez de pedir las cincuenta de golpe: así lo que
    // se está mirando no queda detrás de una cola larga.
    const pedidas = vi
      .mocked(prefetchArtwork)
      .mock.calls.flatMap(([entradas]) => entradas as { appId: number }[]);
    expect(pedidas.length).toBeGreaterThan(0);
    expect(vi.mocked(prefetchArtwork).mock.calls.length).toBeGreaterThan(1);
    expect(pedidas.every((entrada) => entrada.appId >= 1000)).toBe(true);
  });

  it("para al acercarse al techo de la caché en vez de pelearse con el desalojo", async () => {
    const reposo = conReposoInmediato();
    vi.mocked(api.listArtworkTargets).mockResolvedValue(
      Array.from({ length: 50 }, (_, index) => ({
        appId: 2000 + index,
        coverUrl: `https://shared.steamstatic.com/store_item_assets/steam/apps/${2000 + index}/library_600x900_2x.jpg`,
      })),
    );
    // La caché ya roza el presupuesto: seguir adelantando sólo provocaría que
    // el mantenimiento borre justo lo que se acaba de descargar.
    vi.mocked(api.getArtCacheUsage).mockResolvedValue({
      bytes: 500 * 1024 * 1024,
      budgetBytes: 512 * 1024 * 1024,
    });

    renderHook(() => useIdleArtworkPrefetch(true));
    await waitFor(() => expect(api.listArtworkTargets).toHaveBeenCalled());
    await reposo.agotar();
    // La medida se consulta antes de la primera tanda, no después.
    await waitFor(() => expect(api.getArtCacheUsage).toHaveBeenCalled());

    expect(prefetchArtwork).not.toHaveBeenCalled();
  });

  it("no hace nada donde la biblioteca no se ve", async () => {
    conReposoInmediato();
    renderHook(() => useIdleArtworkPrefetch(false));

    await Promise.resolve();
    expect(api.listArtworkTargets).not.toHaveBeenCalled();
    expect(prefetchArtwork).not.toHaveBeenCalled();
  });

  it("un fallo al pedir la lista no molesta a nadie", async () => {
    conReposoInmediato();
    vi.mocked(api.listArtworkTargets).mockRejectedValue(new Error("sin base"));

    // Precargar es una mejora de tiempos, no una función: si falla, la rejilla
    // sigue resolviendo lo suyo al desplazarse y no hay nada que anunciar.
    const { unmount } = renderHook(() => useIdleArtworkPrefetch(true));
    await waitFor(() => expect(api.listArtworkTargets).toHaveBeenCalled());
    expect(prefetchArtwork).not.toHaveBeenCalled();
    expect(() => unmount()).not.toThrow();
  });
});
