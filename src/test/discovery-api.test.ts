import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

describe("contrato Tauri de seguimiento y descubrimiento", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("mantiene snapshot y recordatorios como operaciones persistentes explícitas", async () => {
    const input = {
      appId: 10,
      dueAt: "2026-08-21T18:00:00.000Z",
      note: "Retomar el capítulo",
    };
    await api.discoverySnapshot();
    await api.saveReminder(input);
    await api.snoozeReminder("recordatorio-1", "2026-08-28T18:00:00.000Z");
    await api.completeReminder("recordatorio-1");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_discovery_snapshot");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "save_reminder", { input });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "snooze_reminder", {
      id: "recordatorio-1",
      dueAt: "2026-08-28T18:00:00.000Z",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(4, "complete_reminder", {
      id: "recordatorio-1",
    });
  });

  it("persiste descarte y restauración por el identificador de la recomendación", async () => {
    await api.dismissRecommendation("recomendacion-1");
    await api.restoreRecommendation("recomendacion-1");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "dismiss_recommendation", {
      historyId: "recomendacion-1",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "restore_recommendation", {
      historyId: "recomendacion-1",
    });
  });

  it("actualiza publicaciones oficiales mediante un comando sin clave Web API", async () => {
    await api.refreshDiscoveryNews();

    expect(invokeMock).toHaveBeenCalledWith("refresh_discovery_news");
  });
});
