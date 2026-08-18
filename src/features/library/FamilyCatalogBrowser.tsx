import {
  IconAlertCircle,
  IconBrandSteam,
  IconCircleCheck,
  IconExternalLink,
  IconLoader2,
} from "@tabler/icons-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Artwork, prefetchArtwork } from "@/components/common/Artwork";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { applyScrollEdgeFade, LiquidEdge } from "@/features/library/LiquidEdge";
import {
  DENSITY_METRICS,
  getGridColumns,
  getVirtualGridGeometry,
  useInterfaceDensity,
} from "@/features/shell/interface-density";
import { formatDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  FamilyCatalogAvailability,
  FamilyCatalogGame,
  FamilyCatalogSort,
  LibraryView,
} from "@/lib/types";

interface Props {
  games: FamilyCatalogGame[];
  total: number;
  view: LibraryView;
  availability: FamilyCatalogAvailability;
  sort: FamilyCatalogSort;
  queryKey: string;
  hasMore: boolean;
  loadingMore: boolean;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  onAvailabilityChange: (availability: FamilyCatalogAvailability) => void;
  onSortChange: (sort: FamilyCatalogSort) => void;
  onViewChange: (view: LibraryView) => void;
  onLoadMore: () => void;
  onOpenConfirmed: (appId: number) => void;
}

export function FamilyCatalogBrowser(props: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const [width, setWidth] = useState(900);
  const [feedback, setFeedback] = useState<string>();
  const [openingAppId, setOpeningAppId] = useState<number>();
  const catalogQueryKey = JSON.stringify([
    props.queryKey,
    props.availability,
    props.sort,
    props.view,
  ]);
  const previousCatalogQueryRef = useRef(catalogQueryKey);
  const measuredLayoutRef = useRef<string | undefined>(undefined);
  const grid = props.view === "grid";
  const compact = props.view === "compact";
  const densityMetrics = DENSITY_METRICS[density];
  // La misma cuenta que la biblioteca. La que había aquí no descontaba los
  // huecos entre columnas, así que a igual ancho salían tarjetas de otro tamaño
  // y el catálogo no se parecía al resto de la aplicación.
  const columns = getGridColumns(width + densityMetrics.gridPadding, density);
  // ResizeObserver reports the content box, while the shared geometry helper
  // accepts the scroll container's outer width and subtracts its padding.
  const geometry = getVirtualGridGeometry(
    width + densityMetrics.gridPadding,
    columns,
    props.games.length,
    density,
  );
  // La fila mide lo que mide en la biblioteca. Antes el catálogo se reservaba
  // ciento sesenta píxeles de cuerpo para dos botones y dos párrafos, y por eso
  // sus tarjetas no se parecían a las de al lado.
  const familyGridRowHeight = geometry.rowHeight;
  const rowHeight = compact ? densityMetrics.compactRow : densityMetrics.listRow;
  const virtualLayoutKey = JSON.stringify([
    props.view,
    density,
    columns,
    familyGridRowHeight,
    rowHeight,
  ]);
  const virtualizer = useVirtualizer({
    count: grid ? geometry.rowCount : props.games.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => (grid ? familyGridRowHeight : rowHeight),
    measureElement: (element) => Math.ceil(element.getBoundingClientRect().height),
    overscan: grid ? 4 : 8,
  });
  const rows = virtualizer.getVirtualItems();
  const virtualCount = grid ? geometry.rowCount : props.games.length;
  const loadMoreThreshold = grid ? 2 : 12;
  const lastVirtualIndex = rows.at(-1)?.index;

  useEffect(() => {
    if (props.games.length === 0) return;
    prefetchArtwork(
      props.games.map((game) => ({ appId: game.appId, src: game.coverUrl })),
      "cover",
    );
  }, [props.games]);
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
    if (previousCatalogQueryRef.current === catalogQueryKey) return;
    previousCatalogQueryRef.current = catalogQueryKey;
    if (parentRef.current) parentRef.current.scrollTop = 0;
  }, [catalogQueryKey]);
  useLayoutEffect(() => {
    if (measuredLayoutRef.current === virtualLayoutKey) return;
    measuredLayoutRef.current = virtualLayoutKey;
    virtualizer.measure();
    if (grid) {
      parentRef.current
        ?.querySelectorAll<HTMLElement>(".virtual-grid-row[data-index]")
        .forEach((row) => {
          virtualizer.measureElement(row);
        });
    }
  }, [grid, virtualizer, virtualLayoutKey]);
  useEffect(() => {
    if (
      lastVirtualIndex !== undefined &&
      lastVirtualIndex >= virtualCount - loadMoreThreshold &&
      props.hasMore &&
      !props.loadingMore
    ) {
      props.onLoadMore();
    }
  }, [
    lastVirtualIndex,
    loadMoreThreshold,
    props.hasMore,
    props.loadingMore,
    props.onLoadMore,
    virtualCount,
  ]);

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
      className={`game-browser family-catalog-browser${
        grid
          ? " family-catalog-browser--grid"
          : compact
            ? " family-catalog-browser--list family-catalog-browser--compact"
            : " family-catalog-browser--list"
      }`}
      data-library-surface="true"
      ref={parentRef}
      onScroll={(event) => {
        applyScrollEdgeFade(event.currentTarget);
        props.onScrollOffsetChange?.(event.currentTarget.scrollTop);
      }}
    >
      <LiquidEdge />
      {grid ? (
        <div className="virtual-canvas" style={{ height: virtualizer.getTotalSize() }}>
          {rows.map((row) => (
            <div
              key={row.key}
              ref={virtualizer.measureElement}
              data-index={row.index}
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
                  <FamilyGameCard
                    key={game.appId}
                    game={game}
                    opening={openingAppId === game.appId}
                    onOpenConfirmed={props.onOpenConfirmed}
                    onOpenStore={openStore}
                  />
                ) : null,
              )}
            </div>
          ))}
        </div>
      ) : (
        <>
          <div className="family-list-header" aria-hidden="true">
            <span>JUEGO</span>
            <span>DISPONIBILIDAD</span>
            <span>{compact ? "ACTUALIZADO" : "DESCUBIERTO"}</span>
            <span aria-hidden="true" />
          </div>
          <div className="virtual-canvas" style={{ height: virtualizer.getTotalSize() }}>
            {rows.map((row) => {
              const game = props.games[row.index];
              return game ? (
                <FamilyGameRow
                  key={game.appId}
                  game={game}
                  compact={compact}
                  opening={openingAppId === game.appId}
                  onOpenConfirmed={props.onOpenConfirmed}
                  onOpenStore={openStore}
                  style={{ transform: `translateY(${row.start}px)` }}
                />
              ) : null;
            })}
          </div>
        </>
      )}
      {props.loadingMore && <div className="load-more-indicator">Cargando más juegos…</div>}
      {feedback && (
        <div className="game-action-feedback" data-kind="error" role="alert">
          <IconAlertCircle aria-hidden="true" /> {feedback}
        </div>
      )}
    </div>
  );
}

function FamilyGameCard({ game, opening, onOpenConfirmed, onOpenStore }: FamilyGameItemProps) {
  const confirmado = game.availability === "confirmed";
  return (
    <article className="game-card family-game-card">
      {/* La tarjeta entera abre: la ficha si Steam confirmó el juego, y si no la
          tienda, que es lo único que se puede hacer con él. Igual que en la
          biblioteca, donde el botón es la tarjeta y no un rótulo dentro. */}
      <button
        type="button"
        className="game-card__target"
        disabled={opening}
        aria-label={
          confirmado ? `Abrir ficha de ${game.title}` : `Abrir ${game.title} en la tienda`
        }
        onClick={() => (confirmado ? onOpenConfirmed(game.appId) : onOpenStore(game))}
      >
        <div className="game-card__cover">
          <Artwork appId={game.appId} src={game.coverUrl} title={game.title} />
          <FamilyAvailability game={game} compact={false} />
        </div>
        <div className="game-card__body">
          <div className="game-card__title-row">
            <h3 title={game.title}>{game.title}</h3>
          </div>
          {/* Una sola línea, como la biblioteca. Lo que decían los dos párrafos
              que había aquí ya lo dice el distintivo de la portada. */}
          {/* Una sola línea, como en la biblioteca. El estado no se repite
              aquí: quien lo tiene comprobado ya lleva su marca en la portada. */}
          <div className="game-card__meta">
            <span>Compartido</span>
            <span>·</span>
            <time dateTime={game.updatedAt}>{formatDate(game.updatedAt)}</time>
          </div>
        </div>
      </button>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            className="family-game-card__store"
            size="icon-xs"
            variant="ghost"
            aria-label={`Abrir ${game.title} en la tienda integrada`}
            disabled={opening}
            onClick={() => onOpenStore(game)}
          >
            {opening ? (
              <IconLoader2 className="is-spinning" />
            ) : (
              <IconBrandSteam aria-hidden="true" />
            )}
          </Button>
        </TooltipTrigger>
        <TooltipContent>Tienda integrada</TooltipContent>
      </Tooltip>
    </article>
  );
}

interface FamilyGameItemProps {
  game: FamilyCatalogGame;
  opening: boolean;
  onOpenConfirmed: (appId: number) => void;
  onOpenStore: (game: FamilyCatalogGame) => void;
}

function FamilyGameRow({
  game,
  compact,
  opening,
  onOpenConfirmed,
  onOpenStore,
  style,
}: FamilyGameItemProps & { compact: boolean; style: React.CSSProperties }) {
  return (
    <article className="family-game-row" data-compact={compact} style={style}>
      <div className="family-game-row__identity">
        <Artwork
          appId={game.appId}
          src={game.iconUrl ?? game.coverUrl}
          title={game.title}
          kind="icon"
        />
        <div>
          <strong title={game.title}>{game.title}</strong>
          {!compact && <span>AppID {game.appId}</span>}
        </div>
      </div>
      <FamilyAvailability game={game} compact />
      <time dateTime={compact ? game.updatedAt : game.discoveredAt}>
        {formatDate(compact ? game.updatedAt : game.discoveredAt)}
      </time>
      <FamilyGameActions
        game={game}
        opening={opening}
        compact
        onOpenConfirmed={onOpenConfirmed}
        onOpenStore={onOpenStore}
      />
    </article>
  );
}

/**
 * Marca los juegos que Vindexa ha visto de verdad en este equipo.
 *
 * Sólo aparece en esos. Antes se rotulaban las dos caras —«confirmado
 * localmente» y «por confirmar»— sobre todas las carátulas, y con mil
 * ochocientos juegos prestados eso no era información: era ruido sobre cada
 * portada. Lo normal en un catálogo de Family es no tener la evidencia local,
 * así que lo que merece marca es la excepción, igual que «INSTALADO» en la
 * biblioteca.
 */
function FamilyAvailability({ game, compact }: { game: FamilyCatalogGame; compact: boolean }) {
  if (game.availability !== "confirmed") return null;
  return (
    <span
      className="installed-marker"
      data-compact={compact}
      title="Vindexa ha visto este juego en tu equipo"
    >
      <IconCircleCheck size={12} aria-hidden="true" /> COMPROBADO
    </span>
  );
}

function FamilyGameActions({
  game,
  opening,
  compact = false,
  onOpenConfirmed,
  onOpenStore,
}: FamilyGameItemProps & { compact?: boolean }) {
  return (
    <div className="family-game-card__actions">
      {game.availability === "confirmed" && (
        <Button
          size="xs"
          aria-label={`Abrir ficha de ${game.title}`}
          onClick={() => onOpenConfirmed(game.appId)}
        >
          Abrir ficha
        </Button>
      )}
      <Button
        size={compact ? "icon-xs" : "xs"}
        variant="secondary"
        aria-label={`Abrir ${game.title} en la tienda integrada`}
        title={compact ? "Tienda integrada" : undefined}
        disabled={opening}
        onClick={() => onOpenStore(game)}
      >
        {opening ? <IconLoader2 className="is-spinning" /> : <IconBrandSteam aria-hidden="true" />}
        {!compact && (
          <>
            Tienda integrada <IconExternalLink aria-hidden="true" />
          </>
        )}
      </Button>
    </div>
  );
}
