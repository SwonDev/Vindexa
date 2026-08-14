import {
  IconAlertCircle,
  IconBrandSteam,
  IconCircleCheck,
  IconExternalLink,
  IconLoader2,
} from "@tabler/icons-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useRef, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { Button } from "@/components/ui/button";
import { getVirtualGridGeometry, useInterfaceDensity } from "@/features/shell/interface-density";
import { formatDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { FamilyCatalogGame } from "@/lib/types";

interface Props {
  games: FamilyCatalogGame[];
  total: number;
  hasMore: boolean;
  loadingMore: boolean;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  onLoadMore: () => void;
  onOpenConfirmed: (appId: number) => void;
}

export function FamilyCatalogBrowser(props: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const [width, setWidth] = useState(900);
  const [feedback, setFeedback] = useState<string>();
  const [openingAppId, setOpeningAppId] = useState<number>();
  const columns = Math.max(2, Math.floor((width - 28) / 170));
  const geometry = getVirtualGridGeometry(width, columns, props.games.length, density);
  const virtualizer = useVirtualizer({
    count: geometry.rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => geometry.rowHeight + 24,
    overscan: 2,
  });
  const rows = virtualizer.getVirtualItems();

  useEffect(() => {
    const node = parentRef.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) => entry && setWidth(entry.contentRect.width));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (parentRef.current) parentRef.current.scrollTop = props.initialScrollOffset ?? 0;
  }, [props.initialScrollOffset]);
  useEffect(() => {
    const last = rows.at(-1);
    if (last && last.index >= geometry.rowCount - 2 && props.hasMore && !props.loadingMore) {
      props.onLoadMore();
    }
  }, [geometry.rowCount, props, rows]);

  const openStore = (game: FamilyCatalogGame) => {
    setOpeningAppId(game.appId);
    setFeedback(undefined);
    void api
      .openStore(game.appId)
      .catch((cause) => setFeedback(`${game.title}: ${getErrorMessage(cause)}`))
      .finally(() => setOpeningAppId(undefined));
  };

  return (
    <div
      className="game-browser family-catalog-browser"
      ref={parentRef}
      onScroll={(event) => props.onScrollOffsetChange?.(event.currentTarget.scrollTop)}
    >
      <div className="family-catalog-notice">
        <IconAlertCircle aria-hidden="true" />
        <p>
          Este catálogo combina los juegos visibles de tu grupo. Steam decide la elegibilidad al
          iniciar y puede excluir títulos aunque aparezcan aquí.
        </p>
      </div>
      <div className="virtual-canvas" style={{ height: virtualizer.getTotalSize() }}>
        {rows.map((row) => (
          <div
            key={row.key}
            className="virtual-grid-row"
            style={{
              transform: `translateY(${row.start}px)`,
              gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
            }}
          >
            {Array.from(
              { length: columns },
              (_, column) => props.games[row.index * columns + column],
            ).map((game) =>
              game ? (
                <article className="game-card family-game-card" key={game.appId}>
                  <div className="game-card__cover">
                    <Artwork appId={game.appId} src={game.coverUrl} title={game.title} />
                    <span className="family-availability" data-state={game.availability}>
                      {game.availability === "confirmed" ? (
                        <IconCircleCheck aria-hidden="true" />
                      ) : (
                        <IconAlertCircle aria-hidden="true" />
                      )}
                      {game.availability === "confirmed"
                        ? "CONFIRMADO LOCALMENTE"
                        : "POR CONFIRMAR"}
                    </span>
                  </div>
                  <div className="game-card__body">
                    <div className="game-card__title-row">
                      <h3 title={game.title}>{game.title}</h3>
                    </div>
                    <p className="family-game-card__copy">
                      {game.availability === "confirmed"
                        ? "Detectado por el cliente de Steam."
                        : "Steam comprobará si puede compartirse."}
                    </p>
                    <p className="family-game-card__freshness">
                      Comprobado: {formatDate(game.updatedAt)}
                    </p>
                    <div className="family-game-card__actions">
                      {game.availability === "confirmed" && (
                        <Button size="xs" onClick={() => props.onOpenConfirmed(game.appId)}>
                          Abrir ficha
                        </Button>
                      )}
                      <Button
                        size="xs"
                        variant="secondary"
                        disabled={openingAppId === game.appId}
                        onClick={() => openStore(game)}
                      >
                        {openingAppId === game.appId ? (
                          <IconLoader2 className="is-spinning" />
                        ) : (
                          <IconBrandSteam />
                        )}
                        Tienda integrada <IconExternalLink />
                      </Button>
                    </div>
                  </div>
                </article>
              ) : null,
            )}
          </div>
        ))}
      </div>
      {props.loadingMore && <div className="load-more-indicator">Cargando más juegos…</div>}
      {feedback && (
        <div className="game-action-feedback" data-kind="error" role="alert">
          <IconAlertCircle aria-hidden="true" /> {feedback}
        </div>
      )}
    </div>
  );
}
