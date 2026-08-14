import type { QueryClient } from "@tanstack/react-query";

const STEAM_DERIVED_QUERY_KEYS = [
  ["bootstrap"],
  ["games"],
  ["family-catalog"],
  ["library-filter-options"],
  ["game"],
  ["metadata-enrichment"],
  ["discovery"],
  ["planner-overview"],
  ["planner-add"],
] as const;

/**
 * Invalidates every cached projection that can change after Steam sync or a
 * local manifest import. Prefix matching intentionally refreshes all pages,
 * filters and open game details regardless of their current parameters.
 */
export async function invalidateSteamDerivedQueries(queryClient: QueryClient): Promise<void> {
  await Promise.all(
    STEAM_DERIVED_QUERY_KEYS.map((queryKey) =>
      queryClient.invalidateQueries({ queryKey: [...queryKey] }),
    ),
  );
}
