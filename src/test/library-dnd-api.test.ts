import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/tauri";
import type { LibraryDropReceipt } from "@/lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

describe("comandos públicos de organización por arrastre", () => {
  beforeEach(() => invokeMock.mockResolvedValue(undefined));

  it("envía un lote posicional en una única invocación Tauri", async () => {
    await api.applyLibraryDrop({
      appIds: [30, 40],
      target: { kind: "collection", id: "cooperativos", beforeAppId: 20 },
    });

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("apply_library_drop", {
      input: {
        appIds: [30, 40],
        target: { kind: "collection", id: "cooperativos", beforeAppId: 20 },
      },
    });
  });

  it("devuelve el recibo exacto al comando de deshacer", async () => {
    const receipt: LibraryDropReceipt = {
      kind: "collection",
      operationId: "4b0ac3ae-bac0-478a-b772-8c7f8c5c3c15",
      targetId: "cooperativos",
      appIds: [30],
      beforeAppId: 20,
      previousOrder: [10, 20],
      appliedOrder: [10, 30, 20],
    };

    await api.undoLibraryDrop(receipt);

    expect(invokeMock).toHaveBeenCalledWith("undo_library_drop", { receipt });
  });

  it("persiste el orden completo de colecciones en una sola llamada", async () => {
    await api.reorderCollections(["favoritos", "cooperativos", "pendientes"]);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("reorder_collections", {
      ids: ["favoritos", "cooperativos", "pendientes"],
    });
  });
});
