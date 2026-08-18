import { IconLoader2, IconSearch } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useId, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { PressableSurface } from "@/components/motion";
import { Input } from "@/components/ui/input";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { api, getErrorMessage } from "@/lib/tauri";
import type { GameSummary } from "@/lib/types";

const RESULT_LIMIT = 8;

/**
 * Buscador de un juego de la biblioteca local.
 *
 * Lo comparten los dos lados de la pantalla —añadir un deseado y añadir a una
 * lista curada— porque la operación es idéntica: encontrar un `appId` y
 * devolverlo. Tener dos buscadores distintos habría significado dos umbrales de
 * rebote, dos tamaños de página y dos estados vacíos que decir de otra forma.
 */
export function GamePicker({
  label,
  placeholder,
  disabledAppIds,
  disabledHint,
  busyAppId,
  onPick,
}: {
  label: string;
  placeholder: string;
  /** Juegos que ya están dentro: se muestran, pero no se pueden volver a añadir. */
  disabledAppIds?: ReadonlySet<number> | undefined;
  disabledHint?: string | undefined;
  busyAppId?: number | undefined;
  onPick: (game: GameSummary) => void;
}) {
  const inputId = useId();
  const [query, setQuery] = useState("");
  const debounced = useDebouncedValue(query.trim(), 220);

  const results = useQuery({
    queryKey: ["wishlist-game-picker", debounced],
    queryFn: () => api.listGames({ query: debounced, limit: RESULT_LIMIT, offset: 0 }),
    enabled: debounced.length >= 2,
    staleTime: 30_000,
  });

  const items = results.data?.items ?? [];

  return (
    <div className="game-picker">
      <label className="game-picker__field" htmlFor={inputId}>
        <span className="game-picker__label">{label}</span>
        <span className="game-picker__control">
          <IconSearch aria-hidden="true" />
          <Input
            id={inputId}
            type="search"
            value={query}
            placeholder={placeholder}
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
          {results.isFetching && <IconLoader2 className="is-spinning" aria-hidden="true" />}
        </span>
      </label>
      {debounced.length >= 2 ? (
        results.isError ? (
          <p className="game-picker__note" role="alert">
            {getErrorMessage(results.error)}
          </p>
        ) : !items.length && !results.isPending ? (
          <p className="game-picker__note" role="status">
            Ningún juego de la biblioteca coincide con «{debounced}».
          </p>
        ) : (
          <ul className="game-picker__results">
            {items.map((game) => {
              const already = disabledAppIds?.has(game.appId) ?? false;
              return (
                <li key={game.appId}>
                  <PressableSurface asChild liftPx={1}>
                    <button
                      type="button"
                      className="game-picker__result"
                      data-already={already}
                      disabled={already || busyAppId === game.appId}
                      onClick={() => onPick(game)}
                    >
                      <span className="game-picker__cover" aria-hidden="true">
                        <Artwork
                          appId={game.appId}
                          src={game.coverUrl}
                          title={game.title}
                          kind="cover"
                        />
                      </span>
                      <span className="game-picker__title">{game.title}</span>
                      <span className="game-picker__hint">
                        {already ? (disabledHint ?? "Ya está en la lista") : "Añadir"}
                      </span>
                    </button>
                  </PressableSurface>
                </li>
              );
            })}
          </ul>
        )
      ) : (
        <p className="game-picker__note" role="status">
          Escribe al menos dos letras para buscar en tu biblioteca.
        </p>
      )}
    </div>
  );
}
