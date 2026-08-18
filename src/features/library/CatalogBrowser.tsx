import { IconAlertCircle } from "@tabler/icons-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { ReactNode } from "react";
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
import type { LibraryView } from "@/lib/types";

/**
 * Listado de un catálogo de juegos que no son propiedad de quien usa Vindexa.
 *
 * Hay dos: el de Steam Family y el de las tiendas vinculadas. Los dos enseñan lo
 * mismo —portada, título y una línea— y los dos tienen que comportarse igual que
 * la biblioteca, porque cambiar de sección no puede cambiar cómo se navega.
 *
 * Antes cada uno traía su propia rejilla. El de Family calculaba las columnas
 * con una fórmula que la biblioteca ya había abandonado, no pintaba el fundido
 * de borde y pedía cada portada al montar su tarjeta; el de las tiendas ni
 * siquiera era una rejilla, era una lista de texto dentro de Ajustes. Este
 * componente es el listado, y cada catálogo se limita a decir qué pone en cada
 * tarjeta.
 *
 * Lo que **no** hace: estados, colecciones, progreso ni arrastre. Nada de eso
 * aplica a un juego que no es tuyo, y ofrecerlo diría que sí lo es.
 */

/** Una entrada del catálogo, ya traducida desde su origen. */
export interface CatalogItem {
  /** Identidad estable dentro del catálogo. */
  key: string;
  /**
   * AppID de Steam, si lo hay. Sólo sirve para la caché local del arte: sin él
   * la portada se pinta desde su URL remota, que es lo que toca en una tienda
   * que no es Steam.
   */
  appId?: number | undefined;
  title: string;
  coverUrl?: string | undefined;
  /** Imagen de la vista de lista, más pequeña que la portada. */
  iconUrl?: string | undefined;
  /**
   * Marca sobre la portada. Se reserva para la **excepción**: rotular todas las
   * tarjetas con su estado normal no informa, sólo tapa la imagen.
   */
  badge?: { label: string; hint: string } | undefined;
  /** Línea única bajo el título, como la de la biblioteca. */
  meta: string;
  /** Columnas de la vista de lista, en su orden. */
  columns: readonly string[];
  /** Qué ocurre al pulsar la tarjeta. */
  onOpen?: (() => void) | undefined;
  /** Acción secundaria, en la esquina de la portada. */
  corner?:
    | { label: string; icon: ReactNode; busy?: boolean | undefined; onClick: () => void }
    | undefined;
}

interface CatalogBrowserProps {
  items: readonly CatalogItem[];
  view: LibraryView;
  /**
   * Cambia cuando cambia la consulta. Al cambiar se vuelve al principio: seguir
   * a media altura sobre otro conjunto de resultados desorienta.
   */
  resetKey: string;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  /** Encabezados de la vista de lista, uno por columna de `CatalogItem`. */
  listHeaders: readonly string[];
  /** Distintivo del contenedor, para las hojas de estilo de cada catálogo. */
  surface: string;
  /** Mensaje de error que el propio catálogo quiera mostrar. */
  feedback?: string | undefined;
}

export function CatalogBrowser(props: CatalogBrowserProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const [width, setWidth] = useState(900);
  const previousResetRef = useRef(props.resetKey);
  const measuredLayoutRef = useRef<string | undefined>(undefined);
  const grid = props.view === "grid";
  const compact = props.view === "compact";
  const densityMetrics = DENSITY_METRICS[density];
  // La misma cuenta que la biblioteca: `ResizeObserver` da la caja de contenido
  // y el ayudante compartido espera el ancho exterior, así que se le devuelve el
  // relleno que descuenta por dentro.
  const outerWidth = width + densityMetrics.gridPadding;
  const columns = getGridColumns(outerWidth, density);
  const geometry = getVirtualGridGeometry(outerWidth, columns, props.items.length, density);
  const rowHeight = compact ? densityMetrics.compactRow : densityMetrics.listRow;
  const virtualLayoutKey = JSON.stringify([
    props.view,
    density,
    columns,
    geometry.rowHeight,
    rowHeight,
  ]);
  const virtualizer = useVirtualizer({
    count: grid ? geometry.rowCount : props.items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => (grid ? geometry.rowHeight : rowHeight),
    measureElement: (element) => Math.ceil(element.getBoundingClientRect().height),
    overscan: grid ? 4 : 8,
  });
  const rows = virtualizer.getVirtualItems();
  const virtualCount = grid ? geometry.rowCount : props.items.length;
  const loadMoreThreshold = grid ? 2 : 12;
  const lastVirtualIndex = rows.at(-1)?.index;

  // Las portadas se resuelven con los datos, no con el desplazamiento: cuando la
  // fila entra en pantalla su imagen ya está lista y se pinta en el primer
  // fotograma. Sin esto, la tarjeta pedía la suya al montarse y por eso las
  // portadas «aparecían» al bajar.
  useEffect(() => {
    if (props.items.length === 0) return;
    prefetchArtwork(
      // La caché local se indexa por AppID de Steam. Lo que no lo tiene —una
      // tienda que no es Steam— se pinta desde su URL remota y no hay nada que
      // adelantar.
      props.items.flatMap((item) =>
        item.appId === undefined ? [] : [{ appId: item.appId, src: item.coverUrl }],
      ),
      "cover",
    );
  }, [props.items]);
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
    if (previousResetRef.current === props.resetKey) return;
    previousResetRef.current = props.resetKey;
    if (parentRef.current) parentRef.current.scrollTop = 0;
  }, [props.resetKey]);
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

  const variant = grid ? "grid" : compact ? "compact" : "list";
  return (
    <div
      className={`game-browser catalog-browser catalog-browser--${variant} ${props.surface} ${props.surface}--${variant}`}
      data-library-surface="true"
      data-catalog-view={variant}
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
                (_, column) => props.items[row.index * columns + column],
              ).map((item) => (item ? <CatalogCard key={item.key} item={item} /> : null))}
            </div>
          ))}
        </div>
      ) : (
        <>
          <div className="catalog-list-header" aria-hidden="true">
            {props.listHeaders.map((header) => (
              <span key={header}>{header}</span>
            ))}
            <span aria-hidden="true" />
          </div>
          <div className="virtual-canvas" style={{ height: virtualizer.getTotalSize() }}>
            {rows.map((row) => {
              const item = props.items[row.index];
              return item ? (
                <CatalogRow
                  key={item.key}
                  item={item}
                  compact={compact}
                  style={{ transform: `translateY(${row.start}px)` }}
                />
              ) : null;
            })}
          </div>
        </>
      )}
      {props.loadingMore && <div className="load-more-indicator">Cargando más juegos…</div>}
      {props.feedback && (
        <div className="game-action-feedback" data-kind="error" role="alert">
          <IconAlertCircle aria-hidden="true" /> {props.feedback}
        </div>
      )}
    </div>
  );
}

function CatalogCard({ item }: { item: CatalogItem }) {
  return (
    <article className="game-card catalog-card">
      <button
        type="button"
        className="game-card__target"
        disabled={!item.onOpen}
        aria-label={`Abrir ${item.title}`}
        onClick={() => item.onOpen?.()}
      >
        <div className="game-card__cover">
          <Artwork appId={item.appId} src={item.coverUrl} title={item.title} />
          {item.badge && (
            <span className="installed-marker" title={item.badge.hint}>
              {item.badge.label}
            </span>
          )}
        </div>
        <div className="game-card__body">
          <div className="game-card__title-row">
            <h3 title={item.title}>{item.title}</h3>
          </div>
          <div className="game-card__meta">{item.meta}</div>
        </div>
      </button>
      {item.corner && <CornerAction corner={item.corner} />}
    </article>
  );
}

function CornerAction({ corner }: { corner: NonNullable<CatalogItem["corner"]> }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          className="catalog-card__corner"
          size="icon-xs"
          variant="ghost"
          aria-label={corner.label}
          disabled={corner.busy}
          onClick={corner.onClick}
        >
          {corner.icon}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{corner.label}</TooltipContent>
    </Tooltip>
  );
}

function CatalogRow({
  item,
  compact,
  style,
}: {
  item: CatalogItem;
  compact: boolean;
  style: React.CSSProperties;
}) {
  return (
    <article className="catalog-row" data-compact={compact} style={style}>
      <button
        type="button"
        className="catalog-row__identity"
        disabled={!item.onOpen}
        aria-label={`Abrir ${item.title}`}
        onClick={() => item.onOpen?.()}
      >
        <Artwork
          appId={item.appId}
          src={item.iconUrl ?? item.coverUrl}
          title={item.title}
          kind="icon"
        />
        <strong title={item.title}>{item.title}</strong>
      </button>
      {item.columns.map((value, index) => (
        // El índice es la posición de la columna, que es justo lo que identifica
        // a cada celda: dos columnas pueden tener el mismo texto.
        // biome-ignore lint/suspicious/noArrayIndexKey: la posición es la identidad
        <span key={index}>{value}</span>
      ))}
      {item.corner && <CornerAction corner={item.corner} />}
    </article>
  );
}
