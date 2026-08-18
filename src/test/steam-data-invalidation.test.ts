import type { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { invalidateSteamDerivedQueries } from "@/lib/steam-data-invalidation";

describe("invalidación de datos derivados de Steam", () => {
  it("actualiza todas las superficies que pueden quedar obsoletas tras sincronizar o importar", async () => {
    const invalidateQueries = vi.fn().mockResolvedValue(undefined);

    await invalidateSteamDerivedQueries({ invalidateQueries } as unknown as QueryClient);

    expect(invalidateQueries.mock.calls.map(([filters]) => filters.queryKey)).toEqual([
      ["bootstrap"],
      ["games"],
      ["family-catalog"],
      ["library-filter-options"],
      ["game"],
      ["metadata-enrichment"],
      ["discovery"],
      ["upcoming-releases"],
      ["notification-rules"],
      ["planner-overview"],
      ["planner-add"],
      ["sync-runs"],
    ]);
  });
});
