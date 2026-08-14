import { beforeEach, describe, expect, it, vi } from "vitest";
import { ARTWORK_CACHE_CLEARED_EVENT } from "@/lib/artwork-cache-events";
import { api } from "@/lib/tauri";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

describe("invalidación de caché de artwork", () => {
  beforeEach(() => mocks.invoke.mockReset());

  it("notifica a la UI solo después de que la caché nativa se haya vaciado", async () => {
    const cleared = vi.fn();
    window.addEventListener(ARTWORK_CACHE_CLEARED_EVENT, cleared, { once: true });
    mocks.invoke.mockResolvedValueOnce(undefined);

    await api.clearArtCache();

    expect(mocks.invoke).toHaveBeenCalledWith("clear_art_cache");
    expect(cleared).toHaveBeenCalledOnce();
  });

  it("conserva la memoria frontend si el borrado nativo falla", async () => {
    const cleared = vi.fn();
    window.addEventListener(ARTWORK_CACHE_CLEARED_EVENT, cleared, { once: true });
    mocks.invoke.mockRejectedValueOnce(new Error("fallo nativo"));

    await expect(api.clearArtCache()).rejects.toThrow("fallo nativo");

    expect(cleared).not.toHaveBeenCalled();
    window.removeEventListener(ARTWORK_CACHE_CLEARED_EVENT, cleared);
  });
});
