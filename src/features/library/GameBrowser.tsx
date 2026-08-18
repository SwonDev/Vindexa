import { useDraggable, useDroppable } from "@dnd-kit/core";
import {
  IconAlertCircle,
  IconBrandSteam,
  IconCircleCheck,
  IconDots,
  IconDownload,
  IconExternalLink,
  IconLoader2,
  IconPlayerPlay,
  IconStarFilled,
  IconUsersGroup,
  IconX,
} from "@tabler/icons-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Artwork, prefetchArtwork } from "@/components/common/Artwork";
import type { GameContextAction } from "@/components/common/GameContextMenu";
import { GameContextMenu } from "@/components/common/GameContextMenu";
import { ProgressMeter } from "@/components/common/ProgressMeter";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { listRowBoxHeight, listRowHeight } from "@/features/library/density-list-rows";
import {
  buildLibraryLayout,
  currentGroupAt,
  GROUP_HEADER_HEIGHT,
  type LibraryLayout,
  rowOffsets,
  stickyGroupAt,
} from "@/features/library/group-layout";
import {
  GROUP_RAIL_WIDTH,
  GroupCount,
  GroupRail,
  GroupStickyHeader,
  groupRailVisible,
} from "@/features/library/group-rail";
import { applyScrollEdgeFade, LiquidEdge } from "@/features/library/LiquidEdge";
import { collectionPositionDropId, manualPositionDropId } from "@/features/library/library-dnd";
import {
  groupLibrary,
  type LibraryGroup,
  type LibraryGrouping,
} from "@/features/library/library-grouping";
import { type SelectionGesture, selectionGestureFrom } from "@/features/library/library-selection";
import {
  getGridColumns,
  getVirtualGridGeometry,
  useInterfaceDensity,
} from "@/features/shell/interface-density";
import { onLibraryCommand } from "@/features/shell/shortcuts";
import { formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { CollectionSummary, GameSummary, LibraryView, StatusDefinition } from "@/lib/types";

/**
 * Escribe la intensidad del fundido de borde directamente en el nodo del
 * contenedor desplazable. Evitar el estado de React aquí es deliberado: el
 * scroll llega a 120 eventos por segundo y un renderizado por fotograma haría
 * saltar la lista virtualizada.
 */
type DraggableListeners = ReturnType<typeof useDraggable>["listeners"];

/**
 * `@dnd-kit` expone sus escuchas como `Function` genérica. La biblioteca sólo
 * necesita el gesto de puntero —el resto del arrastre lo gobierna el activador
 * accesible—, así que se acota aquí en un único punto.
 */
function pointerDragListener(
  listeners: DraggableListeners,
): React.PointerEventHandler<HTMLElement> | undefined {
  return listeners?.onPointerDown as React.PointerEventHandler<HTMLElement> | undefined;
}

/** Operaciones de organización que el menú contextual puede ejecutar. */
export interface GameOrganizationHandlers {
  statuses?: readonly StatusDefinition[] | undefined;
  collections?: readonly CollectionSummary[] | undefined;
  collectionIdsByApp?: ReadonlyMap<number, readonly string[]> | undefined;
  onChangeStatus?: ((game: GameSummary, statusId: string) => void) | undefined;
  onChangePriority?: ((game: GameSummary, priority: number) => void) | undefined;
  onToggleCollection?:
    | ((game: GameSummary, collectionId: string, member: boolean) => void)
    | undefined;
  onTogglePinned?: ((game: GameSummary, pinned: boolean) => void) | undefined;
  onToggleTracking?: ((game: GameSummary, tracking: boolean) => void) | undefined;
}

interface GameBrowserProps {
  games: GameSummary[];
  total: number;
  view: LibraryView;
  selected: Set<number>;
  focusedGameId?: number | undefined;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  onSelect: (game: GameSummary, gesture: SelectionGesture) => void;
  /** Mueve el cursor de teclado: solo el navegador conoce filas y columnas. */
  onMoveFocus?: ((appId: number, extend: boolean) => void) | undefined;
  /** Combinaciones vigentes, para que el menú contextual anuncie las reales. */
  shortcuts?: Partial<Record<GameContextAction, string>> | undefined;
  onOpen: (game: GameSummary) => void;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  positionCollectionId?: string | undefined;
  manualPositioning?: boolean | undefined;
  organization?: GameOrganizationHandlers | undefined;
  /** Corte de la lista en grupos con encabezado. `none` la deja continua. */
  grouping?: LibraryGrouping | undefined;
}

export function GameBrowser(props: GameBrowserProps) {
  const [feedback, setFeedback] = useState<ActionFeedback>();
  const runAction: RunGameAction = (game, pendingMessage, successMessage, operation) => {
    setFeedback({ kind: "pending", appId: game.appId, message: pendingMessage });
    void operation()
      .then(() => setFeedback({ kind: "success", appId: game.appId, message: successMessage }))
      .catch((cause) =>
        setFeedback({
          kind: "error",
          appId: game.appId,
          message: `${game.title}: ${getErrorMessage(cause)}`,
        }),
      );
  };
  useEffect(() => {
    if (feedback?.kind !== "success") return;
    const timeout = window.setTimeout(() => setFeedback(undefined), 4_000);
    return () => window.clearTimeout(timeout);
  }, [feedback]);
  const browserProps: InternalGameBrowserProps = {
    ...props,
    runAction,
    actionPending: feedback?.kind === "pending",
  };
  return (
    <>
      {props.view === "grid" ? (
        <VirtualGameGrid {...browserProps} />
      ) : (
        <VirtualGameList {...browserProps} compact={props.view === "compact"} />
      )}
      {feedback && (
        <div
          className="game-action-feedback"
          data-kind={feedback.kind}
          role={feedback.kind === "error" ? "alert" : "status"}
          aria-live={feedback.kind === "error" ? "assertive" : "polite"}
        >
          {feedback.kind === "pending" ? (
            <IconLoader2 className="is-spinning" aria-hidden="true" />
          ) : feedback.kind === "success" ? (
            <IconCircleCheck aria-hidden="true" />
          ) : (
            <IconAlertCircle aria-hidden="true" />
          )}
          <span>{feedback.message}</span>
          {feedback.kind !== "pending" && (
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Cerrar aviso"
              onClick={() => setFeedback(undefined)}
            >
              <IconX />
            </Button>
          )}
        </div>
      )}
    </>
  );
}

interface ActionFeedback {
  kind: "pending" | "success" | "error";
  appId: number;
  message: string;
}

type RunGameAction = (
  game: GameSummary,
  pendingMessage: string,
  successMessage: string,
  operation: () => Promise<unknown>,
) => void;

interface InternalGameBrowserProps extends GameBrowserProps {
  runAction: RunGameAction;
  actionPending: boolean;
}

/**
 * Traduce una orden `moveFocus` del shell a un índice concreto. Vive aquí
 * porque el número de columnas es una consecuencia del ancho medido, y ese dato
 * no sale de este componente.
 */
function useFocusNavigation(props: InternalGameBrowserProps, columns: number) {
  useEffect(() => {
    return onLibraryCommand((command) => {
      if (command.kind !== "moveFocus" || !props.onMoveFocus) return;
      const games = props.games;
      if (!games.length) return;
      const current = games.findIndex((game) => game.appId === props.focusedGameId);
      const step =
        command.direction === "up"
          ? -columns
          : command.direction === "down"
            ? columns
            : command.direction === "left"
              ? -1
              : command.direction === "right"
                ? 1
                : 0;
      const target =
        command.direction === "first"
          ? 0
          : command.direction === "last"
            ? games.length - 1
            : current < 0
              ? 0
              : Math.min(games.length - 1, Math.max(0, current + step));
      const game = games[target];
      if (game) props.onMoveFocus(game.appId, command.extend);
    });
  }, [columns, props]);
}

/**
 * Devuelve la biblioteca a la posición en la que se dejó.
 *
 * Tiene que pasar por el virtualizador: asignar `scrollTop` al contenedor a mano
 * lo mueve sin que él se entere, así que sigue pintando las primeras filas
 * mientras la vista está cientos de píxeles más abajo, y la pantalla se queda en
 * blanco con una fila cortada arriba. Se restaura una sola vez por ámbito,
 * porque cada página nueva cambia el alto total y volver a saltar aquí anularía
 * el desplazamiento que la persona haya hecho entretanto.
 */
function useRestoredScroll(
  scrollRef: React.RefObject<HTMLDivElement | null>,
  virtualizer: { scrollToOffset: (offset: number) => void },
  offset: number | undefined,
) {
  const restored = useRef<number | undefined>(undefined);
  useEffect(() => {
    const target = offset ?? 0;
    if (restored.current === target) return;
    restored.current = target;
    if (target > 0) virtualizer.scrollToOffset(target);
    const node = scrollRef.current;
    if (node) applyScrollEdgeFade(node);
  }, [offset, scrollRef, virtualizer]);
}

/**
 * Medidas del contenedor desplazable: cuánto espacio hay y a qué altura
 * arranca el lienzo virtual dentro de él.
 */
/**
 * Adelanta a la caché las carátulas de la página cargada.
 *
 * Se dispara con los datos, no con el desplazamiento: cuando la fila entra en
 * pantalla su imagen ya está resuelta y se pinta en el primer fotograma. Sin
 * esto, la tarjeta pedía su portada al montarse y por eso «aparecían» al bajar.
 */
function usePrefetchedArtwork(games: readonly GameSummary[], kind: "cover" | "icon") {
  useEffect(() => {
    if (games.length === 0) return;
    prefetchArtwork(
      games.map((game) => ({
        appId: game.appId,
        src: kind === "icon" ? (game.iconUrl ?? game.headerUrl ?? game.coverUrl) : game.coverUrl,
      })),
      kind,
    );
  }, [games, kind]);
}

function useBrowserMetrics(
  scrollRef: React.RefObject<HTMLDivElement | null>,
  canvasRef: React.RefObject<HTMLDivElement | null>,
) {
  const [metrics, setMetrics] = useState({ width: 900, height: 640, canvasTop: 0 });
  useEffect(() => {
    const node = scrollRef.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const { width, height } = entry.contentRect;
      // El observador también dispara con cero mientras el panel se monta o en
      // entornos sin maquetación; conservar la última medida buena evita que
      // las columnas parpadeen y que el raíl reciba una altura negativa.
      if (width <= 0 || height <= 0) return;
      setMetrics({ width, height, canvasTop: canvasRef.current?.offsetTop ?? 0 });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [scrollRef, canvasRef]);
  return metrics;
}

/** Alto máximo del raíl: cabe entre la franja de grupo y el pie del panel. */
function railMaxHeight(containerHeight: number): number {
  return Math.max(72, containerHeight - 108);
}

function useLibraryGroups(games: readonly GameSummary[], grouping: LibraryGrouping | undefined) {
  return useMemo(() => groupLibrary(games, grouping ?? "none"), [games, grouping]);
}

function useLibraryLayout(
  games: readonly GameSummary[],
  groups: readonly LibraryGroup[],
  columns: number,
  rowHeight: number,
): { layout: LibraryLayout; offsets: number[] } {
  const layout = useMemo(
    () => buildLibraryLayout(games, groups, columns),
    [games, groups, columns],
  );
  const offsets = useMemo(() => rowOffsets(layout.rows, rowHeight), [layout.rows, rowHeight]);
  return { layout, offsets };
}

/**
 * El recuento de un encabezado solo puede prometer un total cuando la página
 * cargada es la biblioteca entera; mientras queden páginas por traer habla de
 * lo cargado y lo dice.
 */
function pageIsComplete(props: GameBrowserProps): boolean {
  return !props.hasMore && props.games.length >= props.total;
}

function VirtualGameGrid(props: InternalGameBrowserProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const { width, height, canvasTop } = useBrowserMetrics(parentRef, canvasRef);
  const groups = useLibraryGroups(props.games, props.grouping);
  const railVisible = groupRailVisible(groups.length);
  // El raíl se lleva su ancho de la rejilla: sin descontarlo, las tarjetas
  // quedarían más estrechas que el alto de fila calculado para ellas.
  const usableWidth = Math.max(180, width - (railVisible ? GROUP_RAIL_WIDTH : 0));
  const columns = getGridColumns(usableWidth, density);
  const geometry = getVirtualGridGeometry(usableWidth, columns, props.games.length, density);
  const { layout, offsets } = useLibraryLayout(props.games, groups, columns, geometry.rowHeight);
  const virtualizer = useVirtualizer({
    count: layout.rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (index) =>
      layout.rows[index]?.kind === "header" ? GROUP_HEADER_HEIGHT : geometry.rowHeight,
    overscan: 4,
  });
  const virtualRows = virtualizer.getVirtualItems();
  useRestoredScroll(parentRef, virtualizer, props.initialScrollOffset);
  usePrefetchedArtwork(props.games, "cover");
  useFocusNavigation(props, columns);
  useEffect(() => {
    const row =
      props.focusedGameId === undefined ? undefined : layout.rowOfGame.get(props.focusedGameId);
    // El virtualizador aún puede no tener elemento de scroll en el primer
    // renderizado; desplazar es una cortesía, nunca un requisito.
    if (row !== undefined) virtualizer.scrollToIndex?.(row, { align: "auto" });
  }, [layout, props.focusedGameId, virtualizer]);
  useEffect(() => {
    if (geometry.rowHeight > 0) virtualizer.measure();
  }, [geometry.rowHeight, virtualizer]);
  useEffect(() => {
    const last = virtualRows.at(-1);
    if (last && last.index >= layout.rows.length - 2 && props.hasMore && !props.loadingMore)
      props.onLoadMore();
  }, [props, layout.rows.length, virtualRows]);
  // La rejilla no tiene cabecera de columnas: la franja se clava en el borde
  // superior, así que el desplazamiento hay que llevarlo al origen del lienzo.
  const threshold = (virtualizer.scrollOffset ?? 0) - canvasTop;
  const sticky = stickyGroupAt(layout.jumps, offsets, threshold);
  const stickyRow =
    sticky === undefined ? undefined : layout.rows[layout.jumps[sticky.index]?.row ?? -1];
  const complete = pageIsComplete(props);
  return (
    <div
      className="game-browser"
      data-library-surface="true"
      data-group-rail={railVisible}
      ref={parentRef}
      tabIndex={-1}
      onScroll={(event) => {
        applyScrollEdgeFade(event.currentTarget);
        props.onScrollOffsetChange?.(event.currentTarget.scrollTop);
      }}
    >
      <LiquidEdge />
      {sticky && stickyRow?.kind === "header" && (
        <GroupStickyHeader
          label={stickyRow.label}
          loaded={stickyRow.loaded}
          complete={complete}
          shift={sticky.shift}
        />
      )}
      {railVisible && (
        <GroupRail
          jumps={layout.jumps}
          current={currentGroupAt(layout.jumps, offsets, threshold)}
          maxHeight={railMaxHeight(height)}
          onJump={(row) => virtualizer.scrollToIndex?.(row, { align: "start" })}
        />
      )}
      <div
        ref={canvasRef}
        className="virtual-canvas"
        style={{ height: virtualizer.getTotalSize() }}
      >
        {virtualRows.map((item) => {
          const row = layout.rows[item.index];
          if (!row) return null;
          if (row.kind === "header") {
            // El encabezado del grupo activo ya lo dibuja la franja fija.
            if (layout.groupOfRow[item.index] === sticky?.index) return null;
            return (
              <div
                key={`grupo:${row.key}`}
                className="library-group-header"
                data-group={row.key}
                style={{ height: GROUP_HEADER_HEIGHT, transform: `translateY(${item.start}px)` }}
              >
                <span>{row.label}</span>
                <GroupCount loaded={row.loaded} complete={complete} />
              </div>
            );
          }
          return (
            <div
              key={row.key}
              className="virtual-grid-row"
              style={{
                transform: `translateY(${item.start}px)`,
                gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
              }}
            >
              {row.games.map((game) => (
                <GameCard
                  key={game.appId}
                  game={game}
                  selected={props.selected.has(game.appId)}
                  focused={props.focusedGameId === game.appId}
                  onSelect={props.onSelect}
                  onOpen={props.onOpen}
                  runAction={props.runAction}
                  actionPending={props.actionPending}
                  positionCollectionId={props.positionCollectionId}
                  manualPositioning={props.manualPositioning}
                  organization={props.organization}
                  shortcuts={props.shortcuts}
                />
              ))}
            </div>
          );
        })}
      </div>
      {props.loadingMore && <div className="load-more-indicator">Cargando más juegos…</div>}
    </div>
  );
}

function VirtualGameList(props: InternalGameBrowserProps & { compact: boolean }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const { height } = useBrowserMetrics(parentRef, canvasRef);
  const rowHeight = listRowHeight(density, props.compact);
  const groups = useLibraryGroups(props.games, props.grouping);
  const railVisible = groupRailVisible(groups.length);
  const { layout, offsets } = useLibraryLayout(props.games, groups, 1, rowHeight);
  const virtualizer = useVirtualizer({
    count: layout.rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (index) =>
      layout.rows[index]?.kind === "header" ? GROUP_HEADER_HEIGHT : rowHeight,
    overscan: 8,
  });
  const items = virtualizer.getVirtualItems();
  useRestoredScroll(parentRef, virtualizer, props.initialScrollOffset);
  usePrefetchedArtwork(props.games, "icon");
  useFocusNavigation(props, 1);
  useEffect(() => {
    const row =
      props.focusedGameId === undefined ? undefined : layout.rowOfGame.get(props.focusedGameId);
    if (row !== undefined) virtualizer.scrollToIndex?.(row, { align: "auto" });
  }, [layout, props.focusedGameId, virtualizer]);

  useEffect(() => {
    const last = items.at(-1);
    if (last && last.index >= layout.rows.length - 12 && props.hasMore && !props.loadingMore)
      props.onLoadMore();
  }, [items, layout.rows.length, props]);
  // La franja cuelga de la cabecera de columnas, que es exactamente donde
  // arranca el lienzo: ahí el desplazamiento del contenedor ya es el del lienzo.
  const threshold = virtualizer.scrollOffset ?? 0;
  const sticky = stickyGroupAt(layout.jumps, offsets, threshold);
  const stickyRow =
    sticky === undefined ? undefined : layout.rows[layout.jumps[sticky.index]?.row ?? -1];
  const complete = pageIsComplete(props);
  return (
    <div
      className={`game-browser game-browser--list${props.compact ? " game-browser--compact" : ""}`}
      data-library-surface="true"
      data-group-rail={railVisible}
      ref={parentRef}
      onScroll={(event) => {
        applyScrollEdgeFade(event.currentTarget);
        props.onScrollOffsetChange?.(event.currentTarget.scrollTop);
      }}
    >
      <LiquidEdge />
      <div className="list-header">
        <span>JUEGO</span>
        <span>ESTADO</span>
        <span>PROGRESO</span>
        <span>TIEMPO</span>
        <span>ÚLTIMA VEZ</span>
        <span aria-hidden="true" />
        {sticky && stickyRow?.kind === "header" && (
          <GroupStickyHeader
            label={stickyRow.label}
            loaded={stickyRow.loaded}
            complete={complete}
            shift={sticky.shift}
          />
        )}
        {railVisible && (
          <GroupRail
            jumps={layout.jumps}
            current={currentGroupAt(layout.jumps, offsets, threshold)}
            maxHeight={railMaxHeight(height)}
            onJump={(row) => virtualizer.scrollToIndex?.(row, { align: "start" })}
          />
        )}
      </div>
      {/* Sin margen propio: la cabecera de columnas es `sticky` y ya reserva su
          alto en el flujo, de modo que cualquier separación extra abriría una
          banda muerta entre la cabecera y la primera fila. */}
      <div
        ref={canvasRef}
        className="virtual-canvas"
        style={{ height: virtualizer.getTotalSize(), marginTop: 0 }}
      >
        {items.map((item) => {
          const row = layout.rows[item.index];
          if (!row) return null;
          if (row.kind === "header") {
            if (layout.groupOfRow[item.index] === sticky?.index) return null;
            return (
              <div
                key={`grupo:${row.key}`}
                className="library-group-header"
                data-group={row.key}
                style={{ height: GROUP_HEADER_HEIGHT, transform: `translateY(${item.start}px)` }}
              >
                <span>{row.label}</span>
                <GroupCount loaded={row.loaded} complete={complete} />
              </div>
            );
          }
          const game = row.games[0];
          if (!game) return null;
          return (
            <GameRow
              key={game.appId}
              game={game}
              selected={props.selected.has(game.appId)}
              focused={props.focusedGameId === game.appId}
              onSelect={props.onSelect}
              onOpen={props.onOpen}
              runAction={props.runAction}
              actionPending={props.actionPending}
              positionCollectionId={props.positionCollectionId}
              manualPositioning={props.manualPositioning}
              organization={props.organization}
              shortcuts={props.shortcuts}
              compact={props.compact}
              style={{
                height: listRowBoxHeight(rowHeight),
                transform: `translateY(${item.start}px)`,
              }}
            />
          );
        })}
      </div>
      {props.loadingMore && <div className="load-more-indicator">Cargando más juegos…</div>}
    </div>
  );
}

const GameCard = memo(function GameCard({
  game,
  selected,
  focused,
  onSelect,
  onOpen,
  runAction,
  actionPending,
  positionCollectionId,
  manualPositioning,
  organization,
  shortcuts,
}: {
  game: GameSummary;
  selected: boolean;
  focused: boolean;
  onSelect: GameBrowserProps["onSelect"];
  onOpen: GameBrowserProps["onOpen"];
  runAction: RunGameAction;
  actionPending: boolean;
  positionCollectionId?: string | undefined;
  manualPositioning?: boolean | undefined;
  organization?: GameOrganizationHandlers | undefined;
  shortcuts?: Partial<Record<GameContextAction, string>> | undefined;
}) {
  const {
    attributes,
    listeners,
    setNodeRef: setDragNodeRef,
    setActivatorNodeRef,
    isDragging,
  } = useDraggable({
    id: `game:${game.appId}`,
    data: { appId: game.appId, title: game.title, coverUrl: game.coverUrl },
  });
  const { setNodeRef: setDropNodeRef, isOver } = useDroppable({
    id: positionCollectionId
      ? collectionPositionDropId(positionCollectionId, game.appId)
      : manualPositioning
        ? manualPositionDropId(game.appId)
        : `position-disabled:${game.appId}`,
    disabled: !positionCollectionId && !manualPositioning,
  });
  return (
    <GameActionsMenu
      game={game}
      onOpen={() => onOpen(game)}
      runAction={runAction}
      actionPending={actionPending}
      organization={organization}
      shortcuts={shortcuts}
    >
      <article
        ref={(node) => {
          setDragNodeRef(node);
          setDropNodeRef(node);
        }}
        className="game-card"
        tabIndex={-1}
        onPointerDown={pointerDragListener(listeners)}
        data-selected={selected}
        data-focused={focused}
        data-library-dragging={isDragging}
        data-position-drop={Boolean(positionCollectionId || manualPositioning)}
        data-position-over={isOver}
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          className="game-drag-activator"
          aria-label={`Arrastrar ${game.title}`}
          {...attributes}
          {...listeners}
        />
        <button
          type="button"
          className="game-card__target"
          aria-pressed={selected}
          aria-label={`${game.title}, ${game.statusName}, ${game.progress}%`}
          onClick={(event) => {
            const gesture = selectionGestureFrom(event);
            onSelect(game, gesture);
            // Ampliar la selección no debe abrir la ficha por encima de ella.
            if (gesture === "replace") onOpen(game);
          }}
        >
          <div className="game-card__cover">
            <Artwork appId={game.appId} src={game.coverUrl} title={game.title} />
            {game.installed ? (
              <span className="installed-marker" title="Instalado">
                <IconDownload size={12} /> INSTALADO
              </span>
            ) : game.ownershipSource === "family_shared" ? (
              // Un juego prestado se distingue de uno propio, porque Steam decide
              // la elegibilidad al abrirlo y puede no dejarte jugarlo. Se marca
              // igual que «instalado»: sobre la portada y sólo la excepción.
              <span className="installed-marker" data-family="true" title="Del préstamo familiar">
                <IconUsersGroup size={12} /> FAMILY
              </span>
            ) : null}
          </div>
          <div className="game-card__body">
            <div className="game-card__title-row">
              <h3 title={game.title}>{game.title}</h3>
              {game.rating && (
                <span className="rating" title={`${game.rating} de 10`}>
                  <IconStarFilled size={11} />
                  {game.rating}
                </span>
              )}
            </div>
            <div className="game-card__meta">
              <span className="status-dot" style={{ backgroundColor: game.statusColor }} />
              {game.statusName}
              <span>·</span>
              <span>{formatPlaytime(game.playtimeMinutes)}</span>
            </div>
            <ProgressMeter
              className="game-card__progress"
              value={game.progress}
              label={`Progreso de ${game.title}: ${game.progress}%`}
            />
          </div>
        </button>
        <GameMenu
          game={game}
          onOpen={() => onOpen(game)}
          runAction={runAction}
          actionPending={actionPending}
        />
      </article>
    </GameActionsMenu>
  );
});

const GameRow = memo(function GameRow({
  game,
  selected,
  focused,
  onSelect,
  onOpen,
  runAction,
  actionPending,
  positionCollectionId,
  manualPositioning,
  organization,
  shortcuts,
  compact,
  style,
}: {
  game: GameSummary;
  selected: boolean;
  focused: boolean;
  onSelect: GameBrowserProps["onSelect"];
  onOpen: GameBrowserProps["onOpen"];
  runAction: RunGameAction;
  actionPending: boolean;
  positionCollectionId?: string | undefined;
  manualPositioning?: boolean | undefined;
  organization?: GameOrganizationHandlers | undefined;
  shortcuts?: Partial<Record<GameContextAction, string>> | undefined;
  compact: boolean;
  style: React.CSSProperties;
}) {
  const {
    attributes,
    listeners,
    setNodeRef: setDragNodeRef,
    setActivatorNodeRef,
    isDragging,
  } = useDraggable({
    id: `game:${game.appId}`,
    data: { appId: game.appId, title: game.title, coverUrl: game.coverUrl },
  });
  const { setNodeRef: setDropNodeRef, isOver } = useDroppable({
    id: positionCollectionId
      ? collectionPositionDropId(positionCollectionId, game.appId)
      : manualPositioning
        ? manualPositionDropId(game.appId)
        : `position-disabled:${game.appId}`,
    disabled: !positionCollectionId && !manualPositioning,
  });
  return (
    <GameActionsMenu
      game={game}
      onOpen={() => onOpen(game)}
      runAction={runAction}
      actionPending={actionPending}
      organization={organization}
      shortcuts={shortcuts}
    >
      <article
        ref={(node) => {
          setDragNodeRef(node);
          setDropNodeRef(node);
        }}
        className="game-row"
        tabIndex={-1}
        onPointerDown={pointerDragListener(listeners)}
        data-selected={selected}
        data-focused={focused}
        data-library-dragging={isDragging}
        data-position-drop={Boolean(positionCollectionId || manualPositioning)}
        data-position-over={isOver}
        data-compact={compact}
        // Sólo la traslación del virtualizador. La del arrastre la lleva el
        // acompañante del cursor; sumarlas movía la fila dos veces y el
        // resultado temblaba contra la posición que el virtualizador recalcula.
        style={style}
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          className="game-drag-activator"
          aria-label={`Arrastrar ${game.title}`}
          {...attributes}
          {...listeners}
        />
        <button
          type="button"
          className="game-row__target"
          aria-pressed={selected}
          aria-label={`${game.title}, ${game.statusName}, ${game.progress}%`}
          onClick={(event) => {
            const gesture = selectionGestureFrom(event);
            onSelect(game, gesture);
            // Ampliar la selección no debe abrir la ficha por encima de ella.
            if (gesture === "replace") onOpen(game);
          }}
        >
          <div className="game-row__identity">
            {/* La ultracompacta existe para meter el máximo de títulos en la
                pantalla, así que ahí ni miniatura ni barra dibujada. En el
                resto, `kind` gobierna la geometría del recuadro, no el origen
                de la imagen: se prefiere la cápsula apaisada a la portada
                vertical porque recortar 600×900 a un cuadrado decapita el arte
                mientras que un encabezado conserva el motivo central. */}
            {!compact && (
              <Artwork
                appId={game.appId}
                src={game.iconUrl ?? game.headerUrl ?? game.coverUrl}
                title={game.title}
                kind="icon"
              />
            )}
            <div>
              <strong>{game.title}</strong>
              {!compact && (game.installed || game.isEarlyAccess) && (
                <span>{game.installed ? "Instalado" : "Early Access"}</span>
              )}
            </div>
          </div>
          <div className="game-row__status">
            <i style={{ backgroundColor: game.statusColor }} />
            {game.statusName}
          </div>
          <ProgressMeter
            className="game-row__progress"
            value={game.progress}
            label={`Progreso de ${game.title}: ${game.progress}%`}
            barHidden={compact}
          />
          <span>{formatPlaytime(game.playtimeMinutes)}</span>
          <span>{formatDate(game.lastPlayedAt)}</span>
        </button>
        <GameMenu
          game={game}
          onOpen={() => onOpen(game)}
          runAction={runAction}
          actionPending={actionPending}
        />
      </article>
    </GameActionsMenu>
  );
});

/**
 * Acciones rápidas con el clic derecho. Reutiliza exactamente las mismas
 * operaciones que el menú de tres puntos: una sola definición de lo que puede
 * hacerse con un juego, dos formas de llegar a ella.
 */
function GameActionsMenu({
  game,
  onOpen,
  runAction,
  actionPending,
  organization,
  shortcuts,
  children,
}: {
  game: GameSummary;
  onOpen: () => void;
  runAction: RunGameAction;
  actionPending: boolean;
  organization?: GameOrganizationHandlers | undefined;
  shortcuts?: Partial<Record<GameContextAction, string>> | undefined;
  children: React.ReactNode;
}) {
  const installedActions = game.installed
    ? {
        onPlay: () =>
          runAction(
            game,
            `Abriendo ${game.title}…`,
            `Steam recibió la solicitud para iniciar ${game.title}.`,
            () => api.launchGame(game.appId),
          ),
        onRevealInstallation: () =>
          runAction(
            game,
            "Abriendo la carpeta de instalación…",
            `Se abrió la instalación de ${game.title}.`,
            () => api.revealInstallation(game.appId),
          ),
        onUninstall: () =>
          runAction(
            game,
            `Solicitando la desinstalación de ${game.title}…`,
            `Steam recibió la solicitud para desinstalar ${game.title}.`,
            () => api.uninstallGame(game.appId),
          ),
      }
    : {
        onInstall: () =>
          runAction(
            game,
            `Preparando la instalación de ${game.title}…`,
            `Steam recibió la solicitud para instalar ${game.title}.`,
            () => api.installGame(game.appId),
          ),
      };
  return (
    <GameContextMenu
      game={game}
      busy={actionPending}
      showShortcuts={Boolean(shortcuts)}
      shortcuts={shortcuts}
      onOpenDetail={onOpen}
      onOpenStore={() =>
        runAction(
          game,
          "Abriendo la tienda protegida…",
          `La tienda oficial de ${game.title} se abrió en una sesión privada.`,
          () => api.openStore(game.appId),
        )
      }
      onCopyTitle={() => {
        navigator.clipboard?.writeText(game.title).catch(() => undefined);
      }}
      onCopyAppId={() => {
        navigator.clipboard?.writeText(String(game.appId)).catch(() => undefined);
      }}
      statuses={organization?.statuses}
      collections={organization?.collections}
      collectionIds={organization?.collectionIdsByApp?.get(game.appId)}
      onChangeStatus={organization?.onChangeStatus}
      onChangePriority={organization?.onChangePriority}
      onToggleCollection={organization?.onToggleCollection}
      onTogglePinned={organization?.onTogglePinned}
      onToggleTracking={organization?.onToggleTracking}
      {...installedActions}
    >
      {children}
    </GameContextMenu>
  );
}

function GameMenu({
  game,
  onOpen,
  runAction,
  actionPending,
}: {
  game: GameSummary;
  onOpen: () => void;
  runAction: RunGameAction;
  actionPending: boolean;
}) {
  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-xs"
              className="game-menu"
              aria-label={`Acciones para ${game.title}`}
              onClick={(event) => event.stopPropagation()}
            >
              <IconDots />
            </Button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>Acciones</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" onClick={(event) => event.stopPropagation()}>
        <DropdownMenuItem onClick={onOpen}>
          <IconExternalLink /> Abrir ficha
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {game.installed ? (
          <>
            <DropdownMenuItem
              disabled={actionPending}
              onClick={() =>
                runAction(
                  game,
                  `Abriendo ${game.title}…`,
                  `Steam recibió la solicitud para iniciar ${game.title}.`,
                  () => api.launchGame(game.appId),
                )
              }
            >
              <IconPlayerPlay /> Jugar
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={actionPending}
              onClick={() =>
                runAction(
                  game,
                  "Abriendo la carpeta de instalación…",
                  `Se abrió la instalación de ${game.title}.`,
                  () => api.revealInstallation(game.appId),
                )
              }
            >
              <IconDownload /> Mostrar instalación
            </DropdownMenuItem>
          </>
        ) : (
          <DropdownMenuItem
            disabled={actionPending}
            onClick={() =>
              runAction(
                game,
                `Preparando la instalación de ${game.title}…`,
                `Steam recibió la solicitud para instalar ${game.title}.`,
                () => api.installGame(game.appId),
              )
            }
          >
            <IconDownload /> Instalar con Steam
          </DropdownMenuItem>
        )}
        <DropdownMenuItem
          disabled={actionPending}
          onClick={() =>
            runAction(
              game,
              "Abriendo la tienda protegida…",
              `La tienda oficial de ${game.title} se abrió en una sesión privada.`,
              () => api.openStore(game.appId),
            )
          }
        >
          <IconBrandSteam /> Tienda integrada
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
