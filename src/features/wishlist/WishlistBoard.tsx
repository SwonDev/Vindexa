import {
  type Announcements,
  closestCorners,
  DndContext,
  type DragCancelEvent,
  type DragEndEvent,
  type DragOverEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  type ScreenReaderInstructions,
  useDroppable,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  IconArrowBackUp,
  IconArrowDown,
  IconArrowUp,
  IconCoin,
  IconLoader2,
  IconNote,
  IconPlus,
  IconRefresh,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import {
  AnimatedNumber,
  DragFeedbackSurface,
  type DropState,
  PressableSurface,
  SegmentedControl,
  ShimmerSkeleton,
} from "@/components/motion";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";
import { GamePicker } from "@/features/wishlist/GamePicker";
import { GameVideoPanel } from "@/features/wishlist/GameVideoPanel";
import {
  applyBoardMove,
  bucketMeta,
  describePrice,
  findEntry,
  findEntryBucket,
  formatCents,
  moveWithin,
  normalizeBuckets,
  type PriceLine,
  parsePriceInput,
  parseWishlistBucketDropId,
  parseWishlistDragId,
  priceInputValue,
  summarizePrices,
  WISHLIST_BUCKETS,
  wishlistBucketDropId,
  wishlistDragId,
} from "@/features/wishlist/wishlist-model";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  SaveWishlistEntryInput,
  WishlistBucket,
  WishlistBucketId,
  WishlistEntry,
  WishlistOverview,
  WishlistPriceStatus,
} from "@/lib/types";

const PRIORITY_OPTIONS = [0, 1, 2, 3, 4, 5].map((value) => ({
  value: String(value),
  label: String(value),
  hint: value === 0 ? "Sin prioridad" : `Prioridad ${value} de 5`,
}));

const screenReaderInstructions: ScreenReaderInstructions = {
  draggable:
    "Para mover el juego de carril o cambiar su orden, pulsa espacio o intro para levantarlo, usa las flechas para elegir el destino y vuelve a pulsar espacio o intro para soltarlo. Escape cancela el movimiento.",
};

export function WishlistBoard({
  overview,
  pending,
  error,
  onRetry,
}: {
  overview?: WishlistOverview | undefined;
  pending: boolean;
  error?: unknown;
  onRetry: () => void;
}) {
  const queryClient = useQueryClient();
  const sensors = useSensors(
    // Ocho píxeles: el mismo umbral que la biblioteca. Es lo que permite que
    // toda la tarjeta arrastre sin robarle el clic simple a la selección.
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const [board, setBoard] = useState<WishlistBucket[]>(() => normalizeBuckets(overview));
  const [selectedAppId, setSelectedAppId] = useState<number>();
  const [activeAppId, setActiveAppId] = useState<number>();
  const [message, setMessage] = useState<{ tone: "info" | "error"; text: string }>();
  const [undoBoard, setUndoBoard] = useState<WishlistBucket[]>();

  useEffect(() => {
    setBoard(normalizeBuckets(overview));
  }, [overview]);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["wishlist-overview"] });

  const save = useMutation({
    mutationFn: (input: SaveWishlistEntryInput) => api.saveWishlistEntry(input),
    onSuccess: (entry) => {
      setMessage({ tone: "info", text: `${entry.game.title}: cambios guardados.` });
      void invalidate();
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const remove = useMutation({
    mutationFn: (entry: WishlistEntry) => api.removeWishlistEntry(entry.game.appId),
    onSuccess: (_data, entry) => {
      setSelectedAppId((current) => (current === entry.game.appId ? undefined : current));
      setMessage({ tone: "info", text: `${entry.game.title} salió de los deseados.` });
      void invalidate();
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const move = useMutation({
    mutationFn: ({
      appId,
      bucket,
      beforeAppId,
    }: {
      snapshot: WishlistBucket[];
      appId: number;
      bucket: WishlistBucketId;
      beforeAppId?: number | undefined;
    }) => api.moveWishlistEntry(appId, bucket, beforeAppId),
    onSuccess: (_data, variables) => {
      setUndoBoard(variables.snapshot);
      void invalidate();
    },
    onError: (cause, variables) => {
      setBoard(variables.snapshot);
      setMessage({ tone: "error", text: `No se pudo mover: ${getErrorMessage(cause)}` });
    },
  });

  const reorder = useMutation({
    mutationFn: ({
      bucket,
      ordered,
    }: {
      snapshot: WishlistBucket[];
      bucket: WishlistBucketId;
      ordered: number[];
    }) => api.reorderWishlistBucket(bucket, ordered),
    onSuccess: (_data, variables) => {
      setUndoBoard(variables.snapshot);
      void invalidate();
    },
    onError: (cause, variables) => {
      setBoard(variables.snapshot);
      setMessage({ tone: "error", text: `No se pudo reordenar: ${getErrorMessage(cause)}` });
    },
  });

  const busy = save.isPending || remove.isPending || move.isPending || reorder.isPending;
  const entries = useMemo(() => board.flatMap((bucket) => bucket.items), [board]);
  const selected =
    entries.find((entry) => entry.game.appId === selectedAppId) ?? entries[0] ?? undefined;
  const owned = useMemo(() => new Set(entries.map((entry) => entry.game.appId)), [entries]);

  /* --- Precio observado -------------------------------------------------- */

  /*
   * El precio se pide aparte del tablero a propósito: son dos preguntas con
   * ritmos distintos. El orden de los carriles lo decides tú y está siempre
   * disponible; el precio lo decide Steam, llega más tarde y puede no llegar.
   * Si compartieran consulta, un fallo de red dejaría la lista en blanco.
   */
  const prices = useQuery({
    queryKey: ["wishlist-prices"],
    queryFn: api.wishlistPrices,
  });
  const priceByAppId = useMemo(
    () => new Map((prices.data ?? []).map((status) => [status.appId, status] as const)),
    [prices.data],
  );
  const coverage = useMemo(() => summarizePrices(prices.data ?? []), [prices.data]);

  const refreshPrices = useMutation({
    mutationFn: () => api.refreshWishlistPrices(),
    onSuccess: (report) => {
      const parts = [
        report.observed === 1 ? "1 precio consultado" : `${report.observed} precios consultados`,
      ];
      if (report.changed > 0) parts.push(`${report.changed} cambiaron`);
      if (report.alerts > 0) parts.push(`${report.alerts} bajaron de tu objetivo`);
      // Lo que no se pudo saber también se cuenta: es la mitad honesta del
      // informe y la que explica por qué la lista sigue incompleta.
      if (report.withoutPrice > 0) parts.push(`${report.withoutPrice} sin precio publicado`);
      if (report.failed > 0) parts.push(`${report.failed} fallaron`);
      setMessage({ tone: "info", text: `${parts.join("; ")}.` });
      void queryClient.invalidateQueries({ queryKey: ["wishlist-prices"] });
      void queryClient.invalidateQueries({ queryKey: ["notification-inbox"] });
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  /* --- Operaciones ------------------------------------------------------- */

  const commitMove = (appId: number, bucket: WishlistBucketId, beforeAppId?: number) => {
    const snapshot = board;
    const next = applyBoardMove(board, appId, bucket, beforeAppId);
    setBoard(next);
    move.mutate({ snapshot, appId, bucket, ...(beforeAppId === undefined ? {} : { beforeAppId }) });
  };

  const commitReorder = (bucket: WishlistBucketId, from: number, to: number) => {
    const lane = board.find((candidate) => candidate.bucket === bucket);
    if (!lane || to < 0 || to >= lane.items.length) return;
    const snapshot = board;
    const items = moveWithin(lane.items, from, to);
    setBoard(
      board.map((candidate) =>
        candidate.bucket === bucket ? { ...candidate, items } : { ...candidate },
      ),
    );
    reorder.mutate({ snapshot, bucket, ordered: items.map((item) => item.game.appId) });
  };

  const undoLastMove = () => {
    if (!undoBoard) return;
    const restored = undoBoard;
    setBoard(restored);
    setUndoBoard(undefined);
    setMessage({ tone: "info", text: "Movimiento deshecho." });
    // Se reconstruye carril por carril: deshacer un traslado entre cubos exige
    // devolver la entrada a su cubo antes de recolocar su posición.
    void Promise.all(
      restored.map((lane) =>
        api.reorderWishlistBucket(
          lane.bucket,
          lane.items.map((item) => item.game.appId),
        ),
      ),
    )
      .catch((cause: unknown) =>
        setMessage({ tone: "error", text: `No se pudo deshacer: ${getErrorMessage(cause)}` }),
      )
      .finally(() => void invalidate());
  };

  const addGame = (bucket: WishlistBucketId, appId: number, title: string) => {
    save.mutate(
      { appId, bucket, priority: 0, note: "" },
      {
        onSuccess: () => {
          setSelectedAppId(appId);
          setMessage({ tone: "info", text: `${title} entró en «${bucketMeta(bucket).label}».` });
        },
      },
    );
  };

  /* --- Arrastre ---------------------------------------------------------- */

  const describe = (id: unknown) => {
    const appId = parseWishlistDragId(String(id));
    const entry = appId === undefined ? undefined : findEntry(board, appId);
    const bucket = appId === undefined ? undefined : findEntryBucket(board, appId);
    return { appId, entry, bucket };
  };

  const resolveTarget = (overId: string | number) => {
    const laneId = parseWishlistBucketDropId(overId);
    if (laneId) return { bucket: laneId, beforeAppId: undefined };
    const overAppId = parseWishlistDragId(overId);
    if (overAppId === undefined) return undefined;
    const bucket = findEntryBucket(board, overAppId);
    return bucket ? { bucket, beforeAppId: overAppId } : undefined;
  };

  const announcements: Announcements = {
    onDragStart({ active }: DragStartEvent) {
      const { entry, bucket } = describe(active.id);
      if (!entry || !bucket) return undefined;
      return `Levantado ${entry.game.title}, en ${bucketMeta(bucket).label}.`;
    },
    onDragOver({ active, over }: DragOverEvent) {
      if (!over) return undefined;
      const { entry } = describe(active.id);
      const target = resolveTarget(over.id);
      if (!entry || !target) return undefined;
      return `${entry.game.title} sobre ${bucketMeta(target.bucket).label}.`;
    },
    onDragEnd({ active, over }: DragEndEvent) {
      const { entry } = describe(active.id);
      if (!entry) return undefined;
      if (!over) return `Movimiento de ${entry.game.title} cancelado.`;
      const target = resolveTarget(over.id);
      return target
        ? `${entry.game.title} colocado en ${bucketMeta(target.bucket).label}.`
        : `Movimiento de ${entry.game.title} cancelado.`;
    },
    onDragCancel({ active }: DragCancelEvent) {
      const { entry } = describe(active.id);
      return `Movimiento cancelado${entry ? `; ${entry.game.title} vuelve a su sitio` : ""}.`;
    },
  };

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveAppId(undefined);
    const appId = parseWishlistDragId(active.id);
    if (appId === undefined) return;
    if (!over) {
      setMessage({ tone: "info", text: "Movimiento cancelado; nada cambió." });
      return;
    }
    const target = resolveTarget(over.id);
    if (!target) return;
    const from = findEntryBucket(board, appId);
    if (!from) return;
    if (from === target.bucket) {
      const lane = board.find((candidate) => candidate.bucket === from);
      if (!lane) return;
      const fromIndex = lane.items.findIndex((item) => item.game.appId === appId);
      const toIndex =
        target.beforeAppId === undefined
          ? lane.items.length - 1
          : lane.items.findIndex((item) => item.game.appId === target.beforeAppId);
      if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return;
      commitReorder(from, fromIndex, toIndex);
      return;
    }
    commitMove(appId, target.bucket, target.beforeAppId);
  };

  /* --- Render ------------------------------------------------------------ */

  if (error) {
    return (
      <div className="wishlist-empty" role="alert">
        <strong>No se pudieron leer los deseados</strong>
        {getErrorMessage(error)}
        <Button variant="outline" size="sm" onClick={onRetry}>
          Reintentar
        </Button>
      </div>
    );
  }

  if (pending && !overview) {
    return (
      <div className="wishlist-lanes" aria-busy="true">
        <span className="sr-only" role="status" aria-live="polite">
          Cargando deseados
        </span>
        {WISHLIST_BUCKETS.map((meta) => (
          <div key={meta.id} className="wishlist-lane">
            <div className="wishlist-lane__head">
              <h2 className="wishlist-lane__title">{meta.label}</h2>
            </div>
            <div className="wishlist-lane__body">
              <ShimmerSkeleton count={3} height={62} gapPx={6} radiusPx={2} />
            </div>
          </div>
        ))}
      </div>
    );
  }

  return (
    <div className="wishlist-board">
      {message ? (
        <p
          className="operation-message wishlist-message"
          data-tone={message.tone}
          role={message.tone === "error" ? "alert" : "status"}
        >
          {message.text}
          {undoBoard && message.tone !== "error" && (
            <Button variant="outline" size="xs" onClick={undoLastMove}>
              <IconArrowBackUp /> Deshacer
            </Button>
          )}
        </p>
      ) : (
        <span className="sr-only" role="status" aria-live="polite" />
      )}

      {entries.length > 0 && (
        <div className="wishlist-prices" data-pending={prices.isFetching}>
          <b className="wishlist-prices__figure">{coverage.headline}</b>
          {/* El matiz va en texto, no en color: es lo que impide leer la cifra
              como si la lista estuviera cubierta cuando no lo está. */}
          {coverage.caveat && <span className="wishlist-prices__caveat">{coverage.caveat}</span>}
          {/* El fallo se dice, pero no se anuncia como alerta: la región viva de
              la pantalla es la de operaciones, y duplicarla haría que cada
              recarga interrumpiera a quien usa lector de pantalla. */}
          {prices.isError && (
            <span className="wishlist-prices__caveat">
              No se pudieron leer los precios guardados: {getErrorMessage(prices.error)}
            </span>
          )}
          <Button
            variant="outline"
            size="xs"
            disabled={refreshPrices.isPending}
            onClick={() => refreshPrices.mutate()}
          >
            {refreshPrices.isPending ? (
              <IconLoader2 className="spin" aria-hidden="true" />
            ) : (
              <IconRefresh aria-hidden="true" />
            )}
            {refreshPrices.isPending ? "Consultando a Steam…" : "Actualizar precios"}
          </Button>
        </div>
      )}

      <DndContext
        sensors={sensors}
        collisionDetection={closestCorners}
        accessibility={{ announcements, screenReaderInstructions }}
        onDragStart={({ active }) => setActiveAppId(parseWishlistDragId(active.id))}
        onDragCancel={() => {
          setActiveAppId(undefined);
          setMessage({ tone: "info", text: "Movimiento cancelado; nada cambió." });
        }}
        onDragEnd={onDragEnd}
      >
        <div className="wishlist-lanes">
          {board.map((lane) => (
            <WishlistLane
              key={lane.bucket}
              lane={lane}
              busy={busy}
              dragging={activeAppId !== undefined}
              selectedAppId={selected?.game.appId}
              owned={owned}
              priceByAppId={priceByAppId}
              onSelect={setSelectedAppId}
              onMoveWithin={(index, direction) =>
                commitReorder(lane.bucket, index, index + direction)
              }
              onMoveTo={(appId, bucket) => commitMove(appId, bucket)}
              onRemove={(entry) => remove.mutate(entry)}
              onAdd={(appId, title) => addGame(lane.bucket, appId, title)}
              adding={save.isPending}
            />
          ))}
        </div>
        <DragOverlay dropAnimation={null}>
          {activeAppId !== undefined
            ? (() => {
                const entry = findEntry(board, activeAppId);
                return entry ? (
                  <div className="wishlist-drag-ghost" role="presentation">
                    <span className="wishlist-drag-ghost__cover" aria-hidden="true">
                      <Artwork
                        appId={entry.game.appId}
                        src={entry.game.coverUrl}
                        title={entry.game.title}
                        kind="cover"
                      />
                    </span>
                    <strong>{entry.game.title}</strong>
                  </div>
                ) : null;
              })()
            : null}
        </DragOverlay>
      </DndContext>

      {selected ? (
        <WishlistEntryEditor
          key={selected.game.appId}
          entry={selected}
          busy={busy}
          onSave={(input) => save.mutate(input)}
        />
      ) : (
        <div className="wishlist-empty">
          <strong>Los deseados están vacíos</strong>
          Añade un juego a cualquiera de los cuatro carriles y podrás anotarle un precio objetivo,
          una nota y los vídeos que te ayudaron a decidir.
        </div>
      )}
    </div>
  );
}

/* --- Carril -------------------------------------------------------------- */

function WishlistLane({
  lane,
  busy,
  dragging,
  selectedAppId,
  owned,
  priceByAppId,
  onSelect,
  onMoveWithin,
  onMoveTo,
  onRemove,
  onAdd,
  adding,
}: {
  lane: WishlistBucket;
  busy: boolean;
  dragging: boolean;
  selectedAppId?: number | undefined;
  owned: ReadonlySet<number>;
  priceByAppId: ReadonlyMap<number, WishlistPriceStatus>;
  onSelect: (appId: number) => void;
  onMoveWithin: (index: number, direction: -1 | 1) => void;
  onMoveTo: (appId: number, bucket: WishlistBucketId) => void;
  onRemove: (entry: WishlistEntry) => void;
  onAdd: (appId: number, title: string) => void;
  adding: boolean;
}) {
  const meta = bucketMeta(lane.bucket);
  const { setNodeRef, isOver } = useDroppable({ id: wishlistBucketDropId(lane.bucket) });
  const [pickerOpen, setPickerOpen] = useState(false);
  const dropState: DropState = isOver ? "over" : dragging ? "active" : "idle";

  return (
    <section className="wishlist-lane" aria-label={`${meta.label}: ${meta.hint}`}>
      <header className="wishlist-lane__head">
        <h2 className="wishlist-lane__title">
          {meta.label}
          <span className="wishlist-lane__count">
            <AnimatedNumber value={lane.items.length} />
          </span>
        </h2>
        <p className="wishlist-lane__hint">{meta.hint}</p>
        <Popover open={pickerOpen} onOpenChange={setPickerOpen}>
          <PopoverTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Añadir un juego a ${meta.label}`}
              disabled={adding}
            >
              {adding ? <IconLoader2 className="is-spinning" /> : <IconPlus />}
            </Button>
          </PopoverTrigger>
          <PopoverContent className="wishlist-picker" align="end">
            <GamePicker
              label={`Añadir a ${meta.label}`}
              placeholder="Nombre del juego"
              disabledAppIds={owned}
              disabledHint="Ya está en deseados"
              onPick={(game) => {
                setPickerOpen(false);
                onAdd(game.appId, game.title);
              }}
            />
          </PopoverContent>
        </Popover>
      </header>
      <DragFeedbackSurface
        asChild
        state={dropState}
        hint={dragging ? `Mover a ${meta.label}` : undefined}
      >
        <div className="wishlist-lane__body" ref={setNodeRef}>
          <SortableContext
            items={lane.items.map((entry) => wishlistDragId(entry.game.appId))}
            strategy={verticalListSortingStrategy}
          >
            {lane.items.length ? (
              lane.items.map((entry, index) => (
                <WishlistCard
                  key={entry.game.appId}
                  entry={entry}
                  index={index}
                  total={lane.items.length}
                  bucket={lane.bucket}
                  busy={busy}
                  dragging={dragging}
                  selected={selectedAppId === entry.game.appId}
                  price={describePrice(entry, priceByAppId.get(entry.game.appId))}
                  onSelect={() => onSelect(entry.game.appId)}
                  onMoveWithin={(direction) => onMoveWithin(index, direction)}
                  onMoveTo={(bucket) => onMoveTo(entry.game.appId, bucket)}
                  onRemove={() => onRemove(entry)}
                />
              ))
            ) : (
              <p className="wishlist-lane__empty">{meta.empty}</p>
            )}
          </SortableContext>
        </div>
      </DragFeedbackSurface>
    </section>
  );
}

/* --- Tarjeta ------------------------------------------------------------- */

function WishlistCard({
  entry,
  index,
  total,
  bucket,
  busy,
  dragging,
  selected,
  price,
  onSelect,
  onMoveWithin,
  onMoveTo,
  onRemove,
}: {
  entry: WishlistEntry;
  index: number;
  total: number;
  bucket: WishlistBucketId;
  busy: boolean;
  dragging: boolean;
  selected: boolean;
  price: PriceLine;
  onSelect: () => void;
  onMoveWithin: (direction: -1 | 1) => void;
  onMoveTo: (bucket: WishlistBucketId) => void;
  onRemove: () => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
    isOver,
  } = useSortable({ id: wishlistDragId(entry.game.appId), data: { title: entry.game.title } });
  const dropState: DropState = isDragging ? "idle" : isOver ? "over" : dragging ? "active" : "idle";
  // Cero no es un objetivo: es no haberlo puesto. Enseñarlo como «0,00» junto a
  // «Precio desconocido» daba dos afirmaciones contradictorias en la misma línea.
  const target =
    entry.targetPriceCents === undefined || entry.targetPriceCents <= 0
      ? undefined
      : formatCents(entry.targetPriceCents, entry.currency);

  return (
    <DragFeedbackSurface asChild state={dropState}>
      <article
        ref={setNodeRef}
        className="wishlist-card"
        data-selected={selected}
        data-dragging={isDragging}
        /* Toda la tarjeta arrastra: no hay asa de seis puntos sobre la
           carátula. El activador de abajo existe solo para teclado. */
        onPointerDown={listeners?.onPointerDown as React.PointerEventHandler<HTMLElement>}
        style={{ transform: CSS.Transform.toString(transform), transition }}
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          className="wishlist-drag-activator"
          aria-label={`Arrastrar ${entry.game.title}`}
          {...attributes}
          {...listeners}
        />
        <PressableSurface asChild liftPx={1} hoverScale={1.004}>
          <div className="wishlist-card__surface">
            <span className="wishlist-card__cover" aria-hidden="true">
              <Artwork
                appId={entry.game.appId}
                src={entry.game.coverUrl}
                title={entry.game.title}
                kind="cover"
              />
            </span>
            <h3 className="wishlist-card__name">
              <button
                type="button"
                className="wishlist-card__target"
                aria-pressed={selected}
                onClick={onSelect}
              >
                {entry.game.title}
              </button>
            </h3>
            <p className="wishlist-card__meta">
              {/* Sin prioridad fijada no se dibuja la escala: cinco casillas
                  vacías no dicen nada y en una lista larga son sólo ruido. */}
              {entry.priority > 0 && (
                <span className="wishlist-card__priority" data-level={entry.priority}>
                  <span className="sr-only">Prioridad {entry.priority} de 5</span>
                  {[1, 2, 3, 4, 5].map((step) => (
                    <i key={step} data-on={step <= entry.priority} aria-hidden="true" />
                  ))}
                </span>
              )}
              {target ? (
                <span className="wishlist-card__price">
                  <IconCoin aria-hidden="true" /> {target}
                </span>
              ) : (
                <span className="wishlist-card__price" data-missing="true">
                  Sin precio objetivo
                </span>
              )}
              {entry.note && (
                <span className="wishlist-card__note" title={entry.note}>
                  <IconNote aria-hidden="true" /> {entry.note}
                </span>
              )}
            </p>
            {/* El precio observado y su fecha van juntos y siempre visibles: un
                importe sin la fecha en la que se miró no es un dato utilizable,
                y la ausencia de precio también se enseña. */}
            <p
              className="wishlist-card__observed"
              data-freshness={price.freshness}
              data-meets={price.meetsTarget}
            >
              {price.amount ? (
                <>
                  <b className="wishlist-card__observed-amount">{price.amount}</b>
                  {price.reference && (
                    <s className="wishlist-card__observed-was">{price.reference}</s>
                  )}
                  {price.discount && (
                    <span className="wishlist-card__observed-cut">{price.discount}</span>
                  )}
                </>
              ) : (
                <b className="wishlist-card__observed-amount" data-missing="true">
                  Precio desconocido
                </b>
              )}
              <span className="wishlist-card__observed-when">{price.observed}</span>
              <span className="wishlist-card__observed-verdict">{price.verdict}</span>
              {price.lowest && (
                <span className="wishlist-card__observed-low">
                  Mínimo visto por Vindexa: {price.lowest}
                </span>
              )}
            </p>
          </div>
        </PressableSurface>
        {/* Alternativa completa al gesto: los controles no arrastran nada. */}
        <span className="wishlist-card__actions" onPointerDown={(event) => event.stopPropagation()}>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Subir ${entry.game.title}`}
            disabled={busy || index === 0}
            onClick={onMoveWithin.bind(null, -1)}
          >
            <IconArrowUp />
          </Button>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Bajar ${entry.game.title}`}
            disabled={busy || index === total - 1}
            onClick={onMoveWithin.bind(null, 1)}
          >
            <IconArrowDown />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="xs"
                aria-label={`Mover ${entry.game.title} a otro carril`}
                disabled={busy}
              >
                Mover
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {WISHLIST_BUCKETS.filter((option) => option.id !== bucket).map((option) => (
                <DropdownMenuItem key={option.id} onSelect={() => onMoveTo(option.id)}>
                  {option.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Quitar ${entry.game.title} de los deseados`}
            disabled={busy}
            onClick={onRemove}
          >
            <IconTrash />
          </Button>
        </span>
      </article>
    </DragFeedbackSurface>
  );
}

/* --- Editor de la entrada seleccionada ----------------------------------- */

function WishlistEntryEditor({
  entry,
  busy,
  onSave,
}: {
  entry: WishlistEntry;
  busy: boolean;
  onSave: (input: SaveWishlistEntryInput) => void;
}) {
  const fieldId = useId();
  const [priority, setPriority] = useState(String(entry.priority));
  const [note, setNote] = useState(entry.note);
  const [price, setPrice] = useState(priceInputValue(entry.targetPriceCents));
  const [currency, setCurrency] = useState(entry.currency ?? "EUR");
  const [invalid, setInvalid] = useState(false);

  const submit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const cents = parsePriceInput(price);
    if (cents === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    const code = currency.trim().toUpperCase();
    onSave({
      appId: entry.game.appId,
      bucket: entry.bucket,
      priority: Number(priority),
      note: note.trim(),
      ...(cents === undefined ? {} : { targetPriceCents: cents }),
      ...(cents === undefined || !code ? {} : { currency: code }),
    });
  };

  return (
    <section className="wishlist-editor" aria-label={`Plan de compra de ${entry.game.title}`}>
      <header className="wishlist-editor__head">
        <span className="wishlist-editor__cover" aria-hidden="true">
          <Artwork
            appId={entry.game.appId}
            src={entry.game.coverUrl}
            title={entry.game.title}
            kind="cover"
          />
        </span>
        <p className="wishlist-editor__title">
          <strong>{entry.game.title}</strong>
          <span>{bucketMeta(entry.bucket).label}</span>
        </p>
      </header>
      <form className="wishlist-editor__form" onSubmit={submit}>
        <div className="wishlist-editor__row">
          <SegmentedControl
            size="sm"
            label={`Prioridad de ${entry.game.title}`}
            options={PRIORITY_OPTIONS}
            value={priority}
            onValueChange={setPriority}
          />
        </div>
        <div className="wishlist-editor__row wishlist-editor__row--price">
          <label htmlFor={`${fieldId}-price`}>
            <span>Precio objetivo</span>
            <Input
              id={`${fieldId}-price`}
              inputMode="decimal"
              value={price}
              placeholder="19,99"
              aria-invalid={invalid}
              aria-describedby={invalid ? `${fieldId}-price-error` : undefined}
              onChange={(event) => setPrice(event.currentTarget.value)}
            />
          </label>
          <label htmlFor={`${fieldId}-currency`}>
            <span>Moneda</span>
            <Input
              id={`${fieldId}-currency`}
              value={currency}
              maxLength={3}
              placeholder="EUR"
              onChange={(event) => setCurrency(event.currentTarget.value)}
            />
          </label>
        </div>
        {invalid && (
          <p className="inline-notice" data-kind="error" id={`${fieldId}-price-error`} role="alert">
            <span>Escribe una cantidad con hasta dos decimales, por ejemplo 19,99.</span>
          </p>
        )}
        <label className="wishlist-editor__note" htmlFor={`${fieldId}-note`}>
          <span>Nota</span>
          <Textarea
            id={`${fieldId}-note`}
            rows={2}
            value={note}
            maxLength={500}
            placeholder="Qué esperas de este juego, o qué te frena"
            onChange={(event) => setNote(event.currentTarget.value)}
          />
        </label>
        <div className="wishlist-editor__actions">
          <Button type="submit" size="sm" disabled={busy}>
            {busy && <IconLoader2 className="is-spinning" />} Guardar plan
          </Button>
        </div>
      </form>
      <GameVideoPanel
        appId={entry.game.appId}
        title={entry.game.title}
        headerUrl={entry.game.headerUrl}
        coverUrl={entry.game.coverUrl}
      />
    </section>
  );
}
