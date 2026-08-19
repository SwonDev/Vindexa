import {
  type Announcements,
  closestCenter,
  DndContext,
  type DragCancelEvent,
  type DragEndEvent,
  type DragOverEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  type ScreenReaderInstructions,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  rectSortingStrategy,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  IconArrowBackUp,
  IconArrowDown,
  IconArrowUp,
  IconBolt,
  IconFolder,
  IconLoader2,
  IconPencil,
  IconPlus,
  IconTrash,
  IconX,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { GameContextMenu } from "@/components/common/GameContextMenu";
import { GamePreviewCard } from "@/components/common/GamePreviewCard";
import { PageHeader } from "@/components/common/PageHeader";
import {
  AnimatedNumber,
  DragFeedbackSurface,
  type DropState,
  PressableSurface,
  RevealOnScroll,
  ShimmerSkeleton,
} from "@/components/motion";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  CollectionEditorDialog,
  type CollectionSeed,
  describeSmartRules,
} from "@/features/collections/CollectionEditorDialog";
import { CollectionIcon } from "@/features/collections/CollectionIcon";
import {
  collectionOrderDragId,
  parseCollectionOrderDragId,
  reorderCollectionIds,
} from "@/features/library/library-dnd";
import { CollectionContextMenu } from "@/features/library/SidebarContextMenus";
import {
  type GameQuickActions,
  useGameQuickActions,
} from "@/features/library/use-game-quick-actions";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppBootstrap, CollectionSummary, GameSummary, StatusDefinition } from "@/lib/types";
import "@/features/collections/collections.css";

const GameDetailSheet = lazy(() =>
  import("@/features/library/GameDetailSheet").then((module) => ({
    default: module.GameDetailSheet,
  })),
);

/** Carátulas por tarjeta. Cinco caben en la retícula sin bajar de 40 px de ancho. */
const MOSAIC_LIMIT = 5;
/** Página del panel de detalle. Se amplía bajo demanda, no de golpe. */
const DETAIL_PAGE = 60;
/** Frases de reglas que caben en la tarjeta antes de resumir el resto. */
const SUMMARY_PHRASES = 3;
const RULES_STALE_MS = 120_000;

/*
 * Una biblioteca con cuarenta colecciones no puede abrir cuarenta lecturas a la
 * vez al entrar en la pantalla. Este semáforo mantiene un máximo de peticiones
 * de previsualización en vuelo y encola el resto; junto con la puerta de
 * visibilidad de cada tarjeta, lo que no se ve nunca llega a pedirse.
 */
const PREVIEW_CONCURRENCY = 4;
let activePreviews = 0;
const waitingPreviews: (() => void)[] = [];

function acquirePreviewSlot(): Promise<void> {
  if (activePreviews < PREVIEW_CONCURRENCY) {
    activePreviews += 1;
    return Promise.resolve();
  }
  return new Promise<void>((resolve) => waitingPreviews.push(resolve));
}

function releasePreviewSlot() {
  const next = waitingPreviews.shift();
  // El turno se transfiere en vez de liberarse: el contador no baja mientras
  // haya alguien esperando, así que nunca se superan los turnos permitidos.
  if (next) next();
  else activePreviews -= 1;
}

function withPreviewSlot<T>(task: () => Promise<T>): Promise<T> {
  return acquirePreviewSlot().then(() =>
    task().then(
      (value) => {
        releasePreviewSlot();
        return value;
      },
      (cause: unknown) => {
        releasePreviewSlot();
        throw cause;
      },
    ),
  );
}

/**
 * Plantillas del estado vacío. Cada una abre el editor ya relleno: nada se
 * guarda a espaldas de la persona, y las inteligentes pueden calcular su vista
 * previa antes de crearse. Las que no se pueden expresar con reglas
 * verificables —«Sin DRM», «Cooperativo con amigos»— se ofrecen como manuales
 * en lugar de fingir una automatización que no existe.
 */
const collectionTemplates: readonly (CollectionSeed & { id: string; hint: string })[] = [
  {
    id: "short-sessions",
    name: "Sesiones cortas",
    description: "Menos de una hora por partida, sin hilo argumental que perder.",
    color: "#5CAAC1",
    icon: "sparkles",
    kind: "smart",
    matchMode: "all",
    rules: [{ groupId: 0, field: "estimatedMinutes", operator: "lessOrEqual", value: 60 }],
    hint: "Se mantiene sola con la duración estimada que anotas en cada ficha.",
  },
  {
    id: "half-told",
    name: "Historias a medias",
    description: "Campañas empezadas que merecen un final.",
    color: "#A4D007",
    icon: "bookmark",
    kind: "smart",
    matchMode: "all",
    rules: [
      { groupId: 0, field: "progress", operator: "greaterOrEqual", value: 20 },
      { groupId: 0, field: "progress", operator: "lessOrEqual", value: 80 },
    ],
    hint: "Entra y sale sola según el progreso que registras.",
  },
  {
    id: "drm-free",
    name: "Sin DRM",
    description: "Títulos que puedes conservar y ejecutar sin depender de un cliente.",
    color: "#D6A64B",
    icon: "folder",
    kind: "manual",
    matchMode: "all",
    rules: [],
    hint: "Lista curada: arrastra los juegos desde la biblioteca.",
  },
  {
    id: "coop",
    name: "Cooperativo con amigos",
    description: "Para jugar acompañado sin explicar nada durante media hora.",
    color: "#7EA64B",
    icon: "users",
    kind: "manual",
    matchMode: "all",
    rules: [],
    hint: "Lista curada: arrastra los juegos desde la biblioteca.",
  },
];

function kindLabel(kind: CollectionSummary["kind"]): string {
  return kind === "smart" ? "AUTOMÁTICA" : "MANUAL";
}

function collectionCountLabel(total: number): string {
  return total === 1 ? "1 juego" : `${total.toLocaleString("es-ES")} juegos`;
}

/** Cómo combina sus reglas una colección inteligente. */
function matchLabel(matchMode: CollectionSummary["matchMode"]): string {
  return matchMode === "all" ? "Todas las reglas" : "Cualquier regla";
}

export function CollectionsScreen({
  bootstrap,
  loading,
}: {
  bootstrap?: AppBootstrap;
  loading: boolean;
}) {
  const queryClient = useQueryClient();
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const sourceCollections = bootstrap?.collections ?? [];
  const [collectionOrder, setCollectionOrder] = useState<string[]>(() =>
    sourceCollections.map((collection) => collection.id),
  );
  const [editorCollection, setEditorCollection] = useState<CollectionSummary>();
  const [editorSeed, setEditorSeed] = useState<CollectionSeed>();
  const [editorOpen, setEditorOpen] = useState(false);
  const [message, setMessage] = useState<string>();
  const [activeCollection, setActiveCollection] = useState<CollectionSummary>();
  const [orderUndo, setOrderUndo] = useState<string[]>();
  const [selectedId, setSelectedId] = useState<string>();
  const [openGameId, setOpenGameId] = useState<number>();

  useEffect(() => {
    setCollectionOrder(bootstrap?.collections.map((collection) => collection.id) ?? []);
  }, [bootstrap?.collections]);

  const collections = useMemo(() => {
    const byId = new Map(sourceCollections.map((collection) => [collection.id, collection]));
    const ordered = collectionOrder.flatMap((id) => {
      const collection = byId.get(id);
      return collection ? [collection] : [];
    });
    const missing = sourceCollections.filter(
      (collection) => !collectionOrder.includes(collection.id),
    );
    return [...ordered, ...missing];
  }, [collectionOrder, sourceCollections]);

  // La pantalla nunca se queda sin contenido debajo del tablero: si la
  // selección desaparece —al borrarla, al filtrarla— se adopta la primera de la
  // lista sin necesidad de sincronizar estado en un efecto.
  const selected = collections.find((collection) => collection.id === selectedId) ?? collections[0];

  const deletion = useMutation({
    mutationFn: api.deleteCollection,
    onSuccess: () => {
      setMessage("Colección eliminada.");
      void queryClient.invalidateQueries();
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  const reorder = useMutation({
    mutationFn: ({ next }: { previous: string[]; next: string[] }) => api.reorderCollections(next),
    onSuccess: () => {
      setMessage("Orden de colecciones guardado en este dispositivo.");
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (cause, variables) => {
      setCollectionOrder(variables.previous);
      setMessage(`No se pudo guardar el orden: ${getErrorMessage(cause)}`);
    },
  });
  const removeFromCollection = useMutation({
    mutationFn: ({ game, collectionId }: { game: GameSummary; collectionId: string }) =>
      api.setGameCollections(
        game.appId,
        (game.collectionIds ?? []).filter((id) => id !== collectionId),
      ),
    onSuccess: (_data, variables) => {
      setMessage(`${variables.game.title} salió de la colección.`);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => setMessage(getErrorMessage(cause)),
  });

  /* El clic derecho sobre un juego ofrece aquí lo mismo que en la biblioteca:
     es el mismo juego. Lo propio de esta pantalla —sacarlo de la colección— se
     añade como acción extra, y sólo en las manuales: en una inteligente lo que
     manda son sus reglas. */
  const acciones = useGameQuickActions({
    bootstrap,
    onMessage: setMessage,
    onOpenDetail: (game) => setOpenGameId(game.appId),
  });

  const persistOrder = (next: string[]) => {
    const previous = collections.map((collection) => collection.id);
    if (next === previous || next.every((id, index) => id === previous[index])) return;
    setCollectionOrder(next);
    reorder.mutate({ previous, next }, { onSuccess: () => setOrderUndo(previous) });
  };
  const undoOrderChange = () => {
    if (!orderUndo) return;
    const previous = collections.map((collection) => collection.id);
    setCollectionOrder(orderUndo);
    setOrderUndo(undefined);
    reorder.mutate({ previous, next: orderUndo });
  };
  const moveCollection = (id: string, direction: -1 | 1) => {
    const previous = collections.map((collection) => collection.id);
    const current = previous.indexOf(id);
    const target = current + direction;
    if (current < 0 || target < 0 || target >= previous.length) return;
    persistOrder(reorderCollectionIds(previous, id, previous[target] ?? id));
  };
  const describeCollection = (id: unknown) => {
    const collectionId = parseCollectionOrderDragId(String(id));
    const collection = collections.find((entry) => entry.id === collectionId);
    const position = collection ? collections.indexOf(collection) + 1 : 0;
    return { collection, position };
  };
  const screenReaderInstructions: ScreenReaderInstructions = {
    draggable:
      "Para reordenar la colección, pulsa espacio o intro para levantarla, usa las flechas para elegir la nueva posición y vuelve a pulsar espacio o intro para soltarla. Escape cancela.",
  };
  const announcements: Announcements = {
    onDragStart({ active }: DragStartEvent) {
      const { collection, position } = describeCollection(active.id);
      return collection
        ? `Levantada la colección ${collection.name}, en posición ${position} de ${collections.length}.`
        : undefined;
    },
    onDragOver({ active, over }: DragOverEvent) {
      if (!over) return undefined;
      const { collection } = describeCollection(active.id);
      const { position } = describeCollection(over.id);
      return collection && position
        ? `${collection.name} sobre la posición ${position} de ${collections.length}.`
        : undefined;
    },
    onDragEnd({ active, over }: DragEndEvent) {
      const { collection } = describeCollection(active.id);
      if (!collection) return undefined;
      if (!over) return `Reordenación de ${collection.name} cancelada.`;
      const { position } = describeCollection(over.id);
      return `${collection.name} movida a la posición ${position} de ${collections.length}.`;
    },
    onDragCancel({ active }: DragCancelEvent) {
      const { collection } = describeCollection(active.id);
      return `Reordenación cancelada${collection ? `; ${collection.name} vuelve a su sitio` : ""}.`;
    },
  };
  const onDragStart = ({ active }: DragStartEvent) => {
    const { collection } = describeCollection(active.id);
    setActiveCollection(collection);
  };
  const onDragCancel = () => {
    setActiveCollection(undefined);
    setMessage("Reordenación cancelada; no se cambió la organización.");
  };
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveCollection(undefined);
    if (!over) {
      setMessage("Reordenación cancelada; no se cambió la organización.");
      return;
    }
    const activeId = parseCollectionOrderDragId(active.id);
    const overId = parseCollectionOrderDragId(over.id);
    if (!activeId || !overId) return;
    const previous = collections.map((collection) => collection.id);
    persistOrder(reorderCollectionIds(previous, activeId, overId));
  };
  const openCreate = (seed?: CollectionSeed) => {
    setEditorCollection(undefined);
    setEditorSeed(seed);
    setEditorOpen(true);
  };
  const openEdit = (collection: CollectionSummary) => {
    setEditorCollection(collection);
    setEditorSeed(undefined);
    setEditorOpen(true);
  };

  const busy = deletion.isPending || reorder.isPending;
  const statuses = useMemo(() => bootstrap?.statuses ?? [], [bootstrap?.statuses]);

  return (
    <section className="collections-screen" data-layout="split">
      <PageHeader
        eyebrow="ORGANIZACIÓN TRANSVERSAL"
        title="Colecciones"
        actions={
          <Button onClick={() => openCreate()}>
            <IconPlus /> Nueva colección
          </Button>
        }
      />

      {loading && !bootstrap ? (
        <CollectionsBoardSkeleton />
      ) : !collections.length ? (
        <CollectionsOnboarding onPick={openCreate} />
      ) : (
        <div className="collections-workspace">
          {message ? (
            <p className="operation-message collections-message" role="status">
              {reorder.isPending ? "Guardando orden…" : message}
              {orderUndo && !reorder.isPending && (
                <Button variant="outline" size="xs" onClick={undoOrderChange}>
                  <IconArrowBackUp /> Deshacer
                </Button>
              )}
            </p>
          ) : (
            <span className="sr-only" role="status" aria-live="polite" />
          )}
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            accessibility={{
              announcements,
              screenReaderInstructions,
            }}
            onDragStart={onDragStart}
            onDragEnd={onDragEnd}
            onDragCancel={onDragCancel}
          >
            <div className="collections-board">
              <SortableContext
                items={collections.map((collection) => collectionOrderDragId(collection.id))}
                strategy={rectSortingStrategy}
              >
                <div className="collections-board__grid">
                  {collections.map((collection, index) => (
                    <CollectionTile
                      key={collection.id}
                      collection={collection}
                      index={index}
                      total={collections.length}
                      statuses={statuses}
                      busy={busy}
                      dragging={Boolean(activeCollection)}
                      selected={selected?.id === collection.id}
                      onSelect={() => setSelectedId(collection.id)}
                      onEdit={() => openEdit(collection)}
                      onMove={(direction) => moveCollection(collection.id, direction)}
                      onDelete={() => deletion.mutate(collection.id)}
                    />
                  ))}
                </div>
              </SortableContext>
            </div>
            <DragOverlay dropAnimation={null}>
              {activeCollection ? (
                <div className="collection-drag-ghost" role="presentation">
                  <span
                    className="collection-drag-ghost__accent"
                    style={{ backgroundColor: activeCollection.color }}
                  />
                  <CollectionIcon name={activeCollection.icon} fallback={activeCollection.kind} />
                  <strong>{activeCollection.name}</strong>
                  <data>{activeCollection.gameCount.toLocaleString("es-ES")}</data>
                </div>
              ) : null}
            </DragOverlay>
          </DndContext>
          {selected ? (
            <CollectionDetail
              key={selected.id}
              collection={selected}
              statuses={statuses}
              busy={busy}
              onEdit={() => openEdit(selected)}
              onDelete={() => deletion.mutate(selected.id)}
              onOpenGame={setOpenGameId}
              onRemoveGame={(game) =>
                removeFromCollection.mutate({ game, collectionId: selected.id })
              }
              removing={removeFromCollection.isPending}
              acciones={acciones}
            />
          ) : null}
        </div>
      )}

      <CollectionEditorDialog
        open={editorOpen}
        onOpenChange={(next) => {
          setEditorOpen(next);
          if (!next) {
            setEditorCollection(undefined);
            setEditorSeed(undefined);
          }
        }}
        collection={editorCollection}
        statuses={bootstrap?.statuses}
        seed={editorSeed}
      />
      {openGameId !== undefined && bootstrap ? (
        <Suspense fallback={null}>
          <GameDetailSheet
            appId={openGameId}
            open={openGameId !== undefined}
            onOpenChange={(next) => {
              if (!next) setOpenGameId(undefined);
            }}
            statuses={bootstrap.statuses}
            collections={bootstrap.collections}
            confirmUninstall={bootstrap.preferences.confirmUninstall}
          />
        </Suspense>
      ) : null}
    </section>
  );
}

/* --- Tarjeta de colección ------------------------------------------------ */

function CollectionTile({
  collection,
  index,
  total,
  statuses,
  busy,
  dragging,
  selected,
  onSelect,
  onEdit,
  onMove,
  onDelete,
}: {
  collection: CollectionSummary;
  index: number;
  total: number;
  statuses: readonly StatusDefinition[];
  busy: boolean;
  dragging: boolean;
  selected: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onMove: (direction: -1 | 1) => void;
  onDelete: () => void;
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
  } = useSortable({
    id: collectionOrderDragId(collection.id),
    data: { title: collection.name },
  });
  // La tarjeta solo pide datos cuando ha entrado en pantalla; el resto de la
  // pantalla puede tener cuarenta colecciones sin coste alguno.
  const [revealed, setRevealed] = useState(false);
  // Borrar desde el menú contextual pasa por la misma confirmación que el botón
  // del pie: es la misma acción y no puede ser más fácil por venir de otro sitio.
  const [confirmandoBorrado, setConfirmandoBorrado] = useState(false);

  const preview = useQuery({
    queryKey: ["collection-preview", collection.id, MOSAIC_LIMIT],
    queryFn: () =>
      withPreviewSlot(() =>
        api.listGames({ collectionId: collection.id, limit: MOSAIC_LIMIT, offset: 0 }),
      ),
    enabled: revealed && collection.gameCount > 0,
    staleTime: RULES_STALE_MS,
  });
  const rules = useQuery({
    queryKey: ["collection-rules", collection.id],
    queryFn: () => withPreviewSlot(() => api.listSmartRules(collection.id)),
    enabled: revealed && collection.kind === "smart",
    staleTime: RULES_STALE_MS,
    retry: false,
  });

  const phrases = useMemo(
    () => (rules.data ? describeSmartRules(rules.data, statuses) : []),
    [rules.data, statuses],
  );
  /*
   * La tarjeta abre con lo que la colección significa, no con la maquinaria que
   * la mantiene al día: primero la descripción escrita por la persona y, solo
   * si no hay ninguna, el resumen de reglas. Las reglas completas y su modo de
   * combinación siguen a un palmo —en el `title` de esta línea y en la ficha de
   * la colección—, pero ya no son lo primero que se lee.
   */
  const ruleSummary =
    phrases.length > SUMMARY_PHRASES
      ? `${phrases.slice(0, SUMMARY_PHRASES).join(" · ")} · +${phrases.length - SUMMARY_PHRASES}`
      : phrases.join(" · ");
  const summary =
    collection.description ||
    ruleSummary ||
    (collection.kind === "smart" ? "Se mantiene sola con sus reglas." : "Lista curada a mano.");
  const ruleDetail =
    collection.kind === "smart" && phrases.length
      ? `${matchLabel(collection.matchMode)}: ${phrases.join(" · ")}`
      : "";

  const games = preview.data?.items ?? [];
  const showMosaicSkeleton = preview.isPending && collection.gameCount > 0;
  const empty = collection.gameCount === 0;
  const dropState: DropState = isDragging ? "idle" : isOver ? "over" : dragging ? "active" : "idle";

  return (
    <>
      <CollectionContextMenu
        collection={collection}
        onEdit={onEdit}
        onDelete={() => setConfirmandoBorrado(true)}
      >
        <RevealOnScroll asChild onReveal={() => setRevealed(true)}>
          <DragFeedbackSurface asChild state={dropState}>
            <article
              ref={setNodeRef}
              className="collection-tile"
              data-kind={collection.kind}
              data-selected={selected}
              data-dragging={isDragging}
              data-empty={empty}
              onPointerDown={listeners?.onPointerDown as React.PointerEventHandler<HTMLElement>}
              style={{ transform: CSS.Transform.toString(transform), transition }}
            >
              <span className="collection-tile__accent" aria-hidden="true" />
              <button
                ref={setActivatorNodeRef}
                type="button"
                className="collection-drag-activator"
                aria-label={`Reordenar ${collection.name}`}
                {...attributes}
                {...listeners}
              />
              <PressableSurface asChild liftPx={1} hoverScale={1.004}>
                <div className="collection-tile__surface">
                  {/* El mosaico es una previsualización visual: el nombre, el
                  recuento y el resumen de reglas ya dicen en texto todo lo que
                  contiene, así que leer cinco carátulas seguidas solo añadiría
                  ruido en un lector de pantalla. */}
                  <span className="collection-tile__mosaic" aria-hidden="true">
                    {empty ? (
                      <span className="collection-tile__vacant">
                        <IconPlus />
                        {collection.kind === "smart"
                          ? "Ninguna coincidencia todavía"
                          : "Vacía · arrastra juegos aquí"}
                      </span>
                    ) : showMosaicSkeleton ? (
                      Array.from({ length: MOSAIC_LIMIT }, (_, slot) => (
                        <ShimmerSkeleton
                          // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
                          key={slot}
                          className="collection-tile__cover"
                          aspectRatio="2 / 3"
                          radiusPx={2}
                        />
                      ))
                    ) : (
                      Array.from({ length: MOSAIC_LIMIT }, (_, slot) => {
                        const game = games[slot];
                        return game ? (
                          <span key={game.appId} className="collection-tile__cover">
                            <Artwork
                              appId={game.appId}
                              src={game.coverUrl}
                              title={game.title}
                              kind="cover"
                            />
                          </span>
                        ) : (
                          <span
                            // biome-ignore lint/suspicious/noArrayIndexKey: hueco decorativo de una retícula de longitud fija; no reordena ni guarda estado
                            key={`empty-${slot}`}
                            className="collection-tile__cover collection-tile__cover--empty"
                            aria-hidden="true"
                          />
                        );
                      })
                    )}
                    <span className="collection-tile__badge">
                      {collection.kind === "smart" ? <IconBolt /> : <IconFolder />}
                      {kindLabel(collection.kind)}
                    </span>
                  </span>
                  <span className="collection-tile__body">
                    <span className="collection-tile__icon" aria-hidden="true">
                      <CollectionIcon name={collection.icon} fallback={collection.kind} />
                    </span>
                    <h2 className="collection-tile__name">
                      {/* Sin `aria-label`: el nombre accesible sale del texto, de
                      modo que el encabezado se sigue llamando exactamente como
                      la colección. El recuento vive al lado, en su propia
                      celda, y la selección la comunica `aria-pressed`. */}
                      <button
                        type="button"
                        className="collection-tile__target"
                        aria-pressed={selected}
                        onClick={onSelect}
                      >
                        {collection.name}
                      </button>
                    </h2>
                    <span className="collection-tile__count">
                      <AnimatedNumber value={collection.gameCount} />
                    </span>
                    <p
                      className="collection-tile__summary"
                      data-tone={!collection.description && ruleSummary ? "rules" : "text"}
                      title={ruleDetail}
                    >
                      {summary}
                    </p>
                  </span>
                  {/* El pie ya no repite el tipo de colección —la insignia sobre el
                  mosaico lo dice— ni el modo de combinación de reglas, que se
                  ha ido a la ficha de la colección. Queda como barra de
                  acciones. */}
                  <footer className="collection-tile__footer">
                    {/* Los controles no arrastran la tarjeta: el gesto se detiene aquí. */}
                    <span
                      className="collection-tile__actions"
                      onPointerDown={(event) => event.stopPropagation()}
                    >
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Subir ${collection.name}`}
                        disabled={busy || index === 0}
                        onClick={() => onMove(-1)}
                      >
                        <IconArrowUp />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Bajar ${collection.name}`}
                        disabled={busy || index === total - 1}
                        onClick={() => onMove(1)}
                      >
                        <IconArrowDown />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Editar ${collection.name}`}
                        onClick={onEdit}
                      >
                        <IconPencil />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Eliminar ${collection.name}`}
                        onClick={() => setConfirmandoBorrado(true)}
                      >
                        <IconTrash />
                      </Button>
                    </span>
                  </footer>
                </div>
              </PressableSurface>
            </article>
          </DragFeedbackSurface>
        </RevealOnScroll>
      </CollectionContextMenu>
      <DeleteCollectionDialog
        collection={collection}
        busy={busy}
        onDelete={onDelete}
        open={confirmandoBorrado}
        onOpenChange={setConfirmandoBorrado}
      />
    </>
  );
}

/**
 * La confirmación de borrado, con o sin botón propio.
 *
 * Con `triggerLabel` se dibuja su botón; con `open` la abre quien quiera —el
 * menú contextual, por ejemplo— sin duplicar el texto de la confirmación, que
 * es lo que de verdad importa que sea idéntico venga de donde venga.
 */
function DeleteCollectionDialog({
  collection,
  busy,
  onDelete,
  triggerLabel,
  triggerVariant = "ghost",
  open,
  onOpenChange,
}: {
  collection: CollectionSummary;
  busy: boolean;
  onDelete: () => void;
  triggerLabel?: string | undefined;
  triggerVariant?: "ghost" | "destructive";
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}) {
  const controlado = open !== undefined;
  return (
    <AlertDialog {...(controlado ? { open, ...(onOpenChange ? { onOpenChange } : {}) } : {})}>
      {triggerLabel && (
        <AlertDialogTrigger asChild>
          <Button variant={triggerVariant} size="icon-sm" aria-label={triggerLabel}>
            <IconTrash />
          </Button>
        </AlertDialogTrigger>
      )}
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>¿Eliminar “{collection.name}”?</AlertDialogTitle>
          <AlertDialogDescription>
            La colección desaparecerá, pero ningún juego ni dato personal se eliminará.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancelar</AlertDialogCancel>
          <AlertDialogAction onClick={onDelete} disabled={busy}>
            {busy && <IconLoader2 className="is-spinning" />} Eliminar
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/* --- Panel de detalle ---------------------------------------------------- */

function CollectionDetail({
  collection,
  statuses,
  busy,
  onEdit,
  onDelete,
  onOpenGame,
  onRemoveGame,
  removing,
  acciones,
}: {
  collection: CollectionSummary;
  statuses: readonly StatusDefinition[];
  busy: boolean;
  onEdit: () => void;
  onDelete: () => void;
  onOpenGame: (appId: number) => void;
  onRemoveGame: (game: GameSummary) => void;
  removing: boolean;
  acciones: GameQuickActions;
}) {
  // El panel se remonta con `key={collection.id}`, así que la paginación
  // arranca de cero en cada colección sin necesidad de sincronizarla.
  const [limit, setLimit] = useState(DETAIL_PAGE);

  const games = useQuery({
    queryKey: ["collection-games", collection.id, limit],
    queryFn: () =>
      withPreviewSlot(() => api.listGames({ collectionId: collection.id, limit, offset: 0 })),
    enabled: collection.gameCount > 0,
    staleTime: RULES_STALE_MS,
  });
  const rules = useQuery({
    queryKey: ["collection-rules", collection.id],
    queryFn: () => withPreviewSlot(() => api.listSmartRules(collection.id)),
    enabled: collection.kind === "smart",
    staleTime: RULES_STALE_MS,
    retry: false,
  });
  const phrases = useMemo(
    () => (rules.data ? describeSmartRules(rules.data, statuses) : []),
    [rules.data, statuses],
  );

  const items = games.data?.items ?? [];
  const total = games.data?.total ?? collection.gameCount;
  const smart = collection.kind === "smart";

  return (
    <section
      className="collection-detail"
      data-kind={collection.kind}
      aria-label={`Contenido de ${collection.name}`}
    >
      <header className="collection-detail__head">
        <span className="collection-detail__accent" aria-hidden="true" />
        <span className="collection-detail__icon" aria-hidden="true">
          <CollectionIcon name={collection.icon} fallback={collection.kind} />
        </span>
        {/* Título, no encabezado: la colección seleccionada ya tiene su `h2`
            en la tarjeta del tablero, y repetirlo aquí dejaría dos encabezados
            con el mismo nombre en la misma pantalla. La navegación por regiones
            queda cubierta por el `aria-label` de la sección. */}
        <p className="collection-detail__title">
          <strong>{collection.name}</strong>
          <data value={total}>{collectionCountLabel(total)}</data>
        </p>
        {/* Aquí sí: la ficha de la colección es el sitio donde el modo de
            combinación y las reglas completas responden a una pregunta real
            —«¿por qué está este juego dentro?»—, en vez de rellenar el tablero. */}
        <p className="collection-detail__meta">
          <span className="collection-detail__kind">
            {smart ? <IconBolt /> : <IconFolder />}
            {smart ? "SE MANTIENE SOLA" : "ORDEN MANUAL"}
          </span>
          {smart && phrases.length ? (
            <span>
              {matchLabel(collection.matchMode)}: {phrases.join(" · ")}
            </span>
          ) : collection.description ? (
            <span>{collection.description}</span>
          ) : null}
        </p>
        <div className="collection-detail__actions">
          <Button variant="outline" size="sm" onClick={onEdit}>
            <IconPencil /> Editar
          </Button>
          <DeleteCollectionDialog
            collection={collection}
            busy={busy}
            onDelete={onDelete}
            triggerLabel={`Eliminar la colección ${collection.name}`}
            triggerVariant="destructive"
          />
        </div>
      </header>
      <div className="collection-detail__body">
        {collection.gameCount > 0 && games.isPending ? (
          <ul className="collection-games" aria-busy="true">
            {Array.from({ length: 12 }, (_, slot) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
              <li key={slot} className="collection-games__item collection-games__item--skeleton">
                <ShimmerSkeleton
                  className="collection-games__cover"
                  aspectRatio="2 / 3"
                  radiusPx={2}
                />
                <ShimmerSkeleton height={11} radiusPx={2} />
                <ShimmerSkeleton height={9} width="60%" radiusPx={2} />
              </li>
            ))}
          </ul>
        ) : games.isError ? (
          <p className="collection-detail__empty" role="alert">
            <strong>No se pudo leer el contenido</strong>
            {getErrorMessage(games.error)}
            <Button variant="outline" size="sm" onClick={() => void games.refetch()}>
              Reintentar
            </Button>
          </p>
        ) : !items.length ? (
          <div className="collection-detail__empty">
            <strong>Todavía no hay juegos aquí</strong>
            {smart
              ? "Ninguno cumple las reglas ahora mismo. Ajusta las condiciones para ampliarla."
              : "Arrastra juegos desde la biblioteca o añádelos desde el menú de cada ficha."}
            <Button variant="outline" size="sm" onClick={onEdit}>
              <IconPencil /> {smart ? "Ajustar reglas" : "Editar colección"}
            </Button>
          </div>
        ) : (
          <>
            <ul className="collection-games">
              {items.map((game) => (
                <li key={game.appId} className="collection-games__item">
                  <GameContextMenu
                    game={game}
                    busy={acciones.busy || removing}
                    showShortcuts={false}
                    statuses={acciones.statuses}
                    collections={acciones.collections}
                    collectionIds={acciones.collectionIdsOf(game)}
                    extraActions={
                      smart
                        ? []
                        : [
                            {
                              id: "quitar",
                              label: `Quitar de ${collection.name}`,
                              icon: <IconX aria-hidden="true" />,
                              destructive: true,
                              disabled: removing,
                              onSelect: onRemoveGame,
                            },
                          ]
                    }
                    onOpenDetail={(item) => onOpenGame(item.appId)}
                    onOpenStore={acciones.onOpenStore}
                    onPlay={acciones.onPlay}
                    onInstall={acciones.onInstall}
                    onRevealInstallation={acciones.onRevealInstallation}
                    onChangeStatus={acciones.onChangeStatus}
                    onChangePriority={acciones.onChangePriority}
                    onToggleCollection={acciones.onToggleCollection}
                    onTogglePinned={acciones.onTogglePinned}
                    onToggleTracking={acciones.onToggleTracking}
                    onCopyTitle={acciones.onCopyTitle}
                    onCopyAppId={acciones.onCopyAppId}
                  >
                    {/* Sus capturas al detenerse: dentro de una colección, la
                        fila es una carátula de veinticuatro píxeles y un título. */}
                    <GamePreviewCard
                      appId={game.appId}
                      title={game.title}
                      fallback={
                        <Artwork
                          appId={game.appId}
                          src={game.coverUrl}
                          title={game.title}
                          kind="cover"
                        />
                      }
                      facts={[
                        { label: "Estado", value: game.statusName },
                        { label: "Progreso", value: `${game.progress} %` },
                      ]}
                    >
                      <PressableSurface asChild liftPx={1}>
                        <button
                          type="button"
                          className="collection-games__open"
                          aria-label={`${game.title}, ${game.statusName}, ${game.progress}%`}
                          onClick={() => onOpenGame(game.appId)}
                        >
                          <span className="collection-games__cover" aria-hidden="true">
                            <Artwork
                              appId={game.appId}
                              src={game.coverUrl}
                              title={game.title}
                              kind="cover"
                            />
                          </span>
                          <span className="collection-games__title">{game.title}</span>
                        </button>
                      </PressableSurface>
                    </GamePreviewCard>
                  </GameContextMenu>
                  <span className="collection-games__meta">
                    <span
                      className="collection-games__dot"
                      style={{ backgroundColor: game.statusColor }}
                      aria-hidden="true"
                    />
                    <span>
                      {game.statusName} · {game.progress} %
                    </span>
                    {!smart && (
                      <Button
                        className="collection-games__remove"
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Quitar ${game.title} de ${collection.name}`}
                        disabled={removing}
                        onClick={() => onRemoveGame(game)}
                      >
                        <IconX />
                      </Button>
                    )}
                  </span>
                </li>
              ))}
            </ul>
            {total > items.length && (
              <p className="collection-detail__more" role="status">
                Mostrando {items.length.toLocaleString("es-ES")} de {total.toLocaleString("es-ES")}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={games.isFetching}
                  onClick={() => setLimit((current) => current + DETAIL_PAGE)}
                >
                  {games.isFetching && <IconLoader2 className="is-spinning" />} Mostrar más
                </Button>
              </p>
            )}
          </>
        )}
      </div>
    </section>
  );
}

/* --- Estado vacío y esqueletos ------------------------------------------ */

function CollectionsOnboarding({ onPick }: { onPick: (seed?: CollectionSeed) => void }) {
  return (
    <div className="collections-onboarding">
      <div className="collections-onboarding__intro">
        <h2>Todavía no hay colecciones</h2>
        <p>
          Una colección cruza estados y géneros para responder a una pregunta concreta: qué jugar en
          veinte minutos, qué historia dejaste a medias, qué conservas fuera del cliente. Empieza
          por una de estas o crea la tuya desde cero.
        </p>
      </div>
      <ul className="collections-templates">
        {collectionTemplates.map(({ id, hint, ...seed }) => (
          <li key={id}>
            <button
              type="button"
              className="collection-template"
              data-kind={seed.kind}
              onClick={() => onPick(seed)}
            >
              <span className="collection-template__icon" aria-hidden="true">
                <CollectionIcon name={seed.icon} fallback={seed.kind} />
              </span>
              <span className="collection-template__name">
                {seed.name}
                <span className="collection-template__badge">{kindLabel(seed.kind)}</span>
              </span>
              <span className="collection-template__hint">{hint}</span>
            </button>
          </li>
        ))}
      </ul>
      <div>
        <Button variant="outline" onClick={() => onPick()}>
          <IconPlus /> Empezar en blanco
        </Button>
      </div>
    </div>
  );
}

function CollectionsBoardSkeleton() {
  return (
    <div className="collections-workspace">
      <span className="sr-only" role="status" aria-live="polite">
        Cargando colecciones
      </span>
      <div className="collections-board">
        <div className="collections-board__grid">
          {Array.from({ length: 6 }, (_, tile) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
            <div key={tile} className="collection-tile--skeleton" aria-hidden="true">
              <span className="collection-tile__mosaic">
                {Array.from({ length: MOSAIC_LIMIT }, (_, slot) => (
                  <ShimmerSkeleton
                    // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
                    key={slot}
                    className="collection-tile__cover"
                    aspectRatio="2 / 3"
                    radiusPx={2}
                  />
                ))}
              </span>
              <span className="collection-tile__body">
                <ShimmerSkeleton height={13} width="70%" radiusPx={2} />
                <ShimmerSkeleton height={10} width="90%" radiusPx={2} />
              </span>
              <span className="collection-tile__footer">
                <ShimmerSkeleton height={9} width={92} radiusPx={2} />
              </span>
            </div>
          ))}
        </div>
      </div>
      <div className="collection-detail" />
    </div>
  );
}
