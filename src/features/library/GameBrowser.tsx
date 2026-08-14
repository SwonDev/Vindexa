import { useDraggable, useDroppable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import {
  IconAlertCircle,
  IconBrandSteam,
  IconCircleCheck,
  IconDots,
  IconDownload,
  IconExternalLink,
  IconGripVertical,
  IconLoader2,
  IconPlayerPlay,
  IconStarFilled,
  IconX,
} from "@tabler/icons-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { memo, useEffect, useRef, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { collectionPositionDropId, manualPositionDropId } from "@/features/library/library-dnd";
import {
  DENSITY_METRICS,
  getVirtualGridGeometry,
  useInterfaceDensity,
} from "@/features/shell/interface-density";
import { formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { GameSummary, LibraryView } from "@/lib/types";

interface GameBrowserProps {
  games: GameSummary[];
  total: number;
  view: LibraryView;
  selected: Set<number>;
  focusedGameId?: number | undefined;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  onSelect: (game: GameSummary, additive: boolean) => void;
  onOpen: (game: GameSummary) => void;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  positionCollectionId?: string | undefined;
  manualPositioning?: boolean | undefined;
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

function VirtualGameGrid(props: InternalGameBrowserProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const [width, setWidth] = useState(900);
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
  const columns = Math.max(2, Math.floor((width - 28) / 170));
  const geometry = getVirtualGridGeometry(width, columns, props.games.length, density);
  const rows = geometry.rowCount;
  const estimatedCardHeight = geometry.rowHeight;
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => parentRef.current,
    estimateSize: () => estimatedCardHeight,
    overscan: 2,
  });
  const virtualRows = virtualizer.getVirtualItems();
  useEffect(() => {
    if (estimatedCardHeight > 0) virtualizer.measure();
  }, [estimatedCardHeight, virtualizer]);
  useEffect(() => {
    const last = virtualRows.at(-1);
    if (last && last.index >= rows - 2 && props.hasMore && !props.loadingMore) props.onLoadMore();
  }, [props, rows, virtualRows]);
  return (
    <div
      className="game-browser"
      ref={parentRef}
      tabIndex={-1}
      onScroll={(event) => props.onScrollOffsetChange?.(event.currentTarget.scrollTop)}
    >
      <div className="virtual-canvas" style={{ height: virtualizer.getTotalSize() }}>
        {virtualRows.map((row) => (
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
                />
              ) : null,
            )}
          </div>
        ))}
      </div>
      {props.loadingMore && <div className="load-more-indicator">Cargando más juegos…</div>}
    </div>
  );
}

function VirtualGameList(props: InternalGameBrowserProps & { compact: boolean }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const density = useInterfaceDensity();
  const virtualizer = useVirtualizer({
    count: props.games.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () =>
      props.compact ? DENSITY_METRICS[density].compactRow : DENSITY_METRICS[density].listRow,
    overscan: 8,
  });
  const items = virtualizer.getVirtualItems();
  useEffect(() => {
    if (parentRef.current) parentRef.current.scrollTop = props.initialScrollOffset ?? 0;
  }, [props.initialScrollOffset]);
  useEffect(() => {
    const last = items.at(-1);
    if (last && last.index >= props.games.length - 12 && props.hasMore && !props.loadingMore)
      props.onLoadMore();
  }, [items, props]);
  return (
    <div
      className={`game-browser game-browser--list${props.compact ? " game-browser--compact" : ""}`}
      ref={parentRef}
      onScroll={(event) => props.onScrollOffsetChange?.(event.currentTarget.scrollTop)}
    >
      <div className="list-header">
        <span>JUEGO</span>
        <span>ESTADO</span>
        <span>PROGRESO</span>
        <span>TIEMPO</span>
        <span>ÚLTIMA VEZ</span>
        <span aria-hidden="true" />
      </div>
      <div className="virtual-canvas" style={{ height: virtualizer.getTotalSize() }}>
        {items.map((item) => {
          const game = props.games[item.index];
          return game ? (
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
              compact={props.compact}
              style={{ transform: `translateY(${item.start}px)` }}
            />
          ) : null;
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
}) {
  const {
    attributes,
    listeners,
    setNodeRef: setDragNodeRef,
    setActivatorNodeRef,
    transform,
    isDragging,
  } = useDraggable({
    id: `game:${game.appId}`,
    data: { appId: game.appId, title: game.title },
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
    <article
      ref={(node) => {
        setDragNodeRef(node);
        setDropNodeRef(node);
      }}
      className="game-card"
      data-selected={selected}
      data-focused={focused}
      data-library-dragging={isDragging}
      data-position-drop={Boolean(positionCollectionId || manualPositioning)}
      data-position-over={isOver}
      style={{ transform: CSS.Translate.toString(transform) }}
    >
      <button
        ref={setActivatorNodeRef}
        type="button"
        className="game-drag-handle"
        aria-label={`Arrastrar ${game.title}`}
        title="Arrastrar a un estado, colección o posición"
        {...attributes}
        {...listeners}
      >
        <IconGripVertical aria-hidden="true" />
      </button>
      <button
        type="button"
        className="game-card__target"
        aria-pressed={selected}
        aria-label={`${game.title}, ${game.statusName}, ${game.progress}%`}
        onClick={(event) => {
          const additive = event.metaKey || event.ctrlKey;
          onSelect(game, additive);
          if (!additive) onOpen(game);
        }}
      >
        <div className="game-card__cover">
          <Artwork appId={game.appId} src={game.coverUrl} title={game.title} />
          {game.installed && (
            <span className="installed-marker" title="Instalado">
              <IconDownload size={12} /> INSTALADO
            </span>
          )}
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
          <div className="game-card__progress">
            <Progress
              value={game.progress}
              aria-label={`Progreso de ${game.title}: ${game.progress}%`}
            />
            <span>{game.progress}%</span>
          </div>
        </div>
      </button>
      <GameMenu
        game={game}
        onOpen={() => onOpen(game)}
        runAction={runAction}
        actionPending={actionPending}
      />
    </article>
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
  compact: boolean;
  style: React.CSSProperties;
}) {
  const {
    attributes,
    listeners,
    setNodeRef: setDragNodeRef,
    setActivatorNodeRef,
    transform,
    isDragging,
  } = useDraggable({
    id: `game:${game.appId}`,
    data: { appId: game.appId, title: game.title },
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
    <article
      ref={(node) => {
        setDragNodeRef(node);
        setDropNodeRef(node);
      }}
      className="game-row"
      data-selected={selected}
      data-focused={focused}
      data-library-dragging={isDragging}
      data-position-drop={Boolean(positionCollectionId || manualPositioning)}
      data-position-over={isOver}
      data-compact={compact}
      style={{
        ...style,
        transform: `${String(style.transform ?? "")} ${CSS.Translate.toString(transform) ?? ""}`,
      }}
    >
      <button
        ref={setActivatorNodeRef}
        type="button"
        className="game-drag-handle"
        aria-label={`Arrastrar ${game.title}`}
        title="Arrastrar a un estado, colección o posición"
        {...attributes}
        {...listeners}
      >
        <IconGripVertical aria-hidden="true" />
      </button>
      <button
        type="button"
        className="game-row__target"
        aria-pressed={selected}
        aria-label={`${game.title}, ${game.statusName}, ${game.progress}%`}
        onClick={(event) => {
          const additive = event.metaKey || event.ctrlKey;
          onSelect(game, additive);
          if (!additive) onOpen(game);
        }}
      >
        <div className="game-row__identity">
          <Artwork
            appId={game.appId}
            src={game.iconUrl ?? game.coverUrl}
            title={game.title}
            kind="icon"
          />
          <div>
            <strong>{game.title}</strong>
            {(game.installed || game.isEarlyAccess) && (
              <span>{game.installed ? "Instalado" : "Early Access"}</span>
            )}
          </div>
        </div>
        <div className="game-row__status">
          <i style={{ backgroundColor: game.statusColor }} />
          {game.statusName}
        </div>
        <div className="game-row__progress">
          <Progress value={game.progress} aria-label={`Progreso ${game.progress}%`} />
          <span>{game.progress}%</span>
        </div>
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
  );
});

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
