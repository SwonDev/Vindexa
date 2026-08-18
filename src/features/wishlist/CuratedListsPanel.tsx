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
  IconArrowDown,
  IconArrowUp,
  IconLoader2,
  IconPencil,
  IconPinFilled,
  IconPlus,
  IconStar,
  IconStarFilled,
  IconTrash,
  IconX,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import {
  AnimatedNumber,
  DragFeedbackSurface,
  type DropState,
  PressableSurface,
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { CollectionIcon, collectionIconOptions } from "@/features/collections/CollectionIcon";
import { GamePicker } from "@/features/wishlist/GamePicker";
import {
  accentToken,
  CURATED_ACCENTS,
  CURATED_KINDS,
  curatedKindLabel,
  gameCountLabel,
  moveWithin,
} from "@/features/wishlist/wishlist-model";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  CuratedAccent,
  CuratedList,
  CuratedListKind,
  SaveCuratedListInput,
} from "@/lib/types";

const listDragId = (id: string) => `curated:${encodeURIComponent(id)}`;

function parseListDragId(value: string | number): string | undefined {
  const match = /^curated:(.+)$/.exec(String(value));
  if (!match?.[1]) return undefined;
  try {
    const id = decodeURIComponent(match[1]);
    return id.trim() ? id : undefined;
  } catch {
    return undefined;
  }
}

const screenReaderInstructions: ScreenReaderInstructions = {
  draggable:
    "Para reordenar la lista, pulsa espacio o intro para levantarla, usa las flechas para elegir la nueva posición y vuelve a pulsar espacio o intro para soltarla. Escape cancela.",
};

/**
 * Listas curadas.
 *
 * No son colecciones. Una colección responde a una regla —manual o
 * inteligente— y su valor está en mantenerse sola; una lista curada es una
 * selección editorial con un orden que alguien decidió, una nota por entrada y
 * un puñado de destacados. Por eso aquí no hay reglas ni recuentos automáticos:
 * hay orden, nota y destacado.
 */
export function CuratedListsPanel() {
  const queryClient = useQueryClient();
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const lists = useQuery({ queryKey: ["curated-lists"], queryFn: api.listCuratedLists });
  const [order, setOrder] = useState<string[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [activeId, setActiveId] = useState<string>();
  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<CuratedList>();
  const [message, setMessage] = useState<{ tone: "info" | "error"; text: string }>();

  useEffect(() => {
    setOrder((lists.data ?? []).map((list) => list.id));
  }, [lists.data]);

  const ordered = useMemo(() => {
    const source = lists.data ?? [];
    const byId = new Map(source.map((list) => [list.id, list]));
    const sorted = order.flatMap((id) => {
      const list = byId.get(id);
      return list ? [list] : [];
    });
    return [...sorted, ...source.filter((list) => !order.includes(list.id))];
  }, [lists.data, order]);

  const selected = ordered.find((list) => list.id === selectedId) ?? ordered[0];

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["curated-lists"] });

  const saveList = useMutation({
    mutationFn: (input: SaveCuratedListInput) => api.saveCuratedList(input),
    onSuccess: (list) => {
      setEditorOpen(false);
      setEditing(undefined);
      setSelectedId(list.id);
      setMessage({ tone: "info", text: `Lista «${list.name}» guardada.` });
      void invalidate();
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const deleteList = useMutation({
    mutationFn: (list: CuratedList) => api.deleteCuratedList(list.id),
    onSuccess: (_data, list) => {
      setSelectedId((current) => (current === list.id ? undefined : current));
      setMessage({ tone: "info", text: `Lista «${list.name}» eliminada.` });
      void invalidate();
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const reorderLists = useMutation({
    mutationFn: ({ next }: { previous: string[]; next: string[] }) => api.reorderCuratedLists(next),
    onSuccess: () => void invalidate(),
    onError: (cause, variables) => {
      setOrder(variables.previous);
      setMessage({ tone: "error", text: `No se pudo guardar el orden: ${getErrorMessage(cause)}` });
    },
  });

  const persistOrder = (next: string[]) => {
    const previous = ordered.map((list) => list.id);
    if (next.every((id, index) => id === previous[index])) return;
    setOrder(next);
    reorderLists.mutate({ previous, next });
  };

  const moveList = (id: string, direction: -1 | 1) => {
    const previous = ordered.map((list) => list.id);
    const from = previous.indexOf(id);
    persistOrder(moveWithin(previous, from, from + direction));
  };

  const describeList = (id: unknown) => {
    const listId = parseListDragId(String(id));
    const list = ordered.find((candidate) => candidate.id === listId);
    return { list, position: list ? ordered.indexOf(list) + 1 : 0 };
  };

  const announcements: Announcements = {
    onDragStart({ active }: DragStartEvent) {
      const { list, position } = describeList(active.id);
      return list
        ? `Levantada la lista ${list.name}, en posición ${position} de ${ordered.length}.`
        : undefined;
    },
    onDragOver({ active, over }: DragOverEvent) {
      if (!over) return undefined;
      const { list } = describeList(active.id);
      const { position } = describeList(over.id);
      return list && position
        ? `${list.name} sobre la posición ${position} de ${ordered.length}.`
        : undefined;
    },
    onDragEnd({ active, over }: DragEndEvent) {
      const { list } = describeList(active.id);
      if (!list) return undefined;
      if (!over) return `Reordenación de ${list.name} cancelada.`;
      const { position } = describeList(over.id);
      return `${list.name} movida a la posición ${position} de ${ordered.length}.`;
    },
    onDragCancel({ active }: DragCancelEvent) {
      const { list } = describeList(active.id);
      return `Reordenación cancelada${list ? `; ${list.name} vuelve a su sitio` : ""}.`;
    },
  };

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveId(undefined);
    if (!over) return;
    const from = parseListDragId(active.id);
    const to = parseListDragId(over.id);
    if (!from || !to || from === to) return;
    const previous = ordered.map((list) => list.id);
    persistOrder(moveWithin(previous, previous.indexOf(from), previous.indexOf(to)));
  };

  if (lists.isError) {
    return (
      <div className="wishlist-empty" role="alert">
        <strong>No se pudieron leer las listas curadas</strong>
        {getErrorMessage(lists.error)}
        <Button variant="outline" size="sm" onClick={() => void lists.refetch()}>
          Reintentar
        </Button>
      </div>
    );
  }

  return (
    <div className="curated-panel">
      {message ? (
        <p
          className="operation-message wishlist-message"
          data-tone={message.tone}
          role={message.tone === "error" ? "alert" : "status"}
        >
          {message.text}
        </p>
      ) : (
        <span className="sr-only" role="status" aria-live="polite" />
      )}

      <div className="curated-panel__toolbar">
        <p className="curated-panel__intro">
          Selecciones editoriales tuyas: orden decidido a mano, una nota por juego y los destacados
          que quieras. No se mantienen solas; ese es justamente el punto.
        </p>
        <Button
          size="sm"
          onClick={() => {
            setEditing(undefined);
            setEditorOpen(true);
          }}
        >
          <IconPlus /> Nueva lista
        </Button>
      </div>

      {lists.isPending ? (
        <div className="curated-board" aria-busy="true">
          <span className="sr-only" role="status" aria-live="polite">
            Cargando listas curadas
          </span>
          {Array.from({ length: 3 }, (_, tile) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
            <div key={tile} className="curated-tile curated-tile--skeleton" aria-hidden="true">
              <ShimmerSkeleton height={14} width="60%" radiusPx={2} />
              <ShimmerSkeleton height={10} width="85%" radiusPx={2} />
            </div>
          ))}
        </div>
      ) : !ordered.length ? (
        <div className="wishlist-empty">
          <strong>Todavía no hay ninguna lista curada</strong>
          Una lista curada es una recomendación tuya con forma: «lo que enseñaría a alguien que
          nunca ha jugado a un metroidvania», en el orden en que lo enseñarías.
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setEditing(undefined);
              setEditorOpen(true);
            }}
          >
            <IconPlus /> Crear la primera
          </Button>
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          accessibility={{ announcements, screenReaderInstructions }}
          onDragStart={({ active }) => setActiveId(parseListDragId(active.id))}
          onDragCancel={() => setActiveId(undefined)}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={ordered.map((list) => listDragId(list.id))}
            strategy={rectSortingStrategy}
          >
            <div className="curated-board">
              {ordered.map((list, index) => (
                <CuratedTile
                  key={list.id}
                  list={list}
                  index={index}
                  total={ordered.length}
                  dragging={Boolean(activeId)}
                  selected={selected?.id === list.id}
                  busy={reorderLists.isPending || deleteList.isPending}
                  onSelect={() => setSelectedId(list.id)}
                  onEdit={() => {
                    setEditing(list);
                    setEditorOpen(true);
                  }}
                  onMove={(direction) => moveList(list.id, direction)}
                  onDelete={() => deleteList.mutate(list)}
                />
              ))}
            </div>
          </SortableContext>
          <DragOverlay dropAnimation={null}>
            {activeId ? (
              <div className="curated-drag-ghost" role="presentation">
                {ordered.find((list) => list.id === activeId)?.name}
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      )}

      {selected ? <CuratedListDetail key={selected.id} list={selected} /> : null}

      <CuratedListEditor
        open={editorOpen}
        list={editing}
        pending={saveList.isPending}
        onOpenChange={(next) => {
          setEditorOpen(next);
          if (!next) setEditing(undefined);
        }}
        onSubmit={(input) => saveList.mutate(input)}
      />
    </div>
  );
}

/* --- Tarjeta de lista ---------------------------------------------------- */

function CuratedTile({
  list,
  index,
  total,
  dragging,
  selected,
  busy,
  onSelect,
  onEdit,
  onMove,
  onDelete,
}: {
  list: CuratedList;
  index: number;
  total: number;
  dragging: boolean;
  selected: boolean;
  busy: boolean;
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
  } = useSortable({ id: listDragId(list.id), data: { title: list.name } });
  const dropState: DropState = isDragging ? "idle" : isOver ? "over" : dragging ? "active" : "idle";

  return (
    <DragFeedbackSurface asChild state={dropState}>
      <article
        ref={setNodeRef}
        className="curated-tile"
        data-kind={list.kind}
        data-selected={selected}
        data-dragging={isDragging}
        onPointerDown={listeners?.onPointerDown as React.PointerEventHandler<HTMLElement>}
        style={{ transform: CSS.Transform.toString(transform), transition }}
      >
        <span
          className="curated-tile__accent"
          style={{ backgroundColor: accentToken(list.accent) }}
          aria-hidden="true"
        />
        <button
          ref={setActivatorNodeRef}
          type="button"
          className="wishlist-drag-activator"
          aria-label={`Reordenar ${list.name}`}
          {...attributes}
          {...listeners}
        />
        <PressableSurface asChild liftPx={1} hoverScale={1.004}>
          <div className="curated-tile__surface">
            <span className="curated-tile__icon" aria-hidden="true">
              <CollectionIcon name={list.icon} fallback="manual" />
            </span>
            <h3 className="curated-tile__name">
              <button
                type="button"
                className="curated-tile__target"
                aria-pressed={selected}
                onClick={onSelect}
              >
                {list.name}
              </button>
            </h3>
            <span className="curated-tile__count">
              <AnimatedNumber value={list.gameCount} />
            </span>
            <p className="curated-tile__summary">
              {list.description || "Sin descripción todavía."}
            </p>
            <footer className="curated-tile__footer">
              <span className="curated-tile__chip">{curatedKindLabel(list.kind)}</span>
              {list.pinned && (
                <span className="curated-tile__chip" data-pinned="true">
                  <IconPinFilled aria-hidden="true" /> FIJADA
                </span>
              )}
            </footer>
          </div>
        </PressableSurface>
        <span className="curated-tile__actions" onPointerDown={(event) => event.stopPropagation()}>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Subir ${list.name}`}
            disabled={busy || index === 0}
            onClick={() => onMove(-1)}
          >
            <IconArrowUp />
          </Button>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Bajar ${list.name}`}
            disabled={busy || index === total - 1}
            onClick={() => onMove(1)}
          >
            <IconArrowDown />
          </Button>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Editar ${list.name}`}
            onClick={onEdit}
          >
            <IconPencil />
          </Button>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="ghost" size="icon-xs" aria-label={`Eliminar ${list.name}`}>
                <IconTrash />
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>¿Eliminar «{list.name}»?</AlertDialogTitle>
                <AlertDialogDescription>
                  Desaparecerá la lista y su orden. Ningún juego ni dato personal se borra.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancelar</AlertDialogCancel>
                <AlertDialogAction onClick={onDelete} disabled={busy}>
                  Eliminar
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </span>
      </article>
    </DragFeedbackSurface>
  );
}

/* --- Detalle de la lista ------------------------------------------------- */

function CuratedListDetail({ list }: { list: CuratedList }) {
  const queryClient = useQueryClient();
  const [message, setMessage] = useState<string>();
  const [noteDraft, setNoteDraft] = useState<{ appId: number; note: string }>();

  const detail = useQuery({
    queryKey: ["curated-list-detail", list.id],
    queryFn: () => api.curatedListDetail(list.id),
  });

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["curated-list-detail", list.id] });
    void queryClient.invalidateQueries({ queryKey: ["curated-lists"] });
  };

  const add = useMutation({
    mutationFn: (appId: number) =>
      api.addCuratedGame({ listId: list.id, appId, note: "", highlight: false }),
    onSuccess: () => {
      setMessage("Juego añadido al final de la lista.");
      invalidate();
    },
    onError: (cause) => setMessage(getErrorMessage(cause)),
  });

  const update = useMutation({
    mutationFn: ({ appId, note, highlight }: { appId: number; note: string; highlight: boolean }) =>
      api.updateCuratedItem({ listId: list.id, appId, note, highlight }),
    onSuccess: () => {
      setNoteDraft(undefined);
      setMessage("Entrada actualizada.");
      invalidate();
    },
    onError: (cause) => setMessage(getErrorMessage(cause)),
  });

  const removeItem = useMutation({
    mutationFn: (appId: number) => api.removeCuratedGame(list.id, appId),
    onSuccess: () => {
      setMessage("Juego retirado de la lista.");
      invalidate();
    },
    onError: (cause) => setMessage(getErrorMessage(cause)),
  });

  const reorderItems = useMutation({
    mutationFn: (ordered: number[]) => api.reorderCuratedItems(list.id, ordered),
    onSuccess: invalidate,
    onError: (cause) => setMessage(getErrorMessage(cause)),
  });

  const items = detail.data?.items ?? [];
  const present = useMemo(() => new Set(items.map((item) => item.game.appId)), [items]);
  const busy = add.isPending || update.isPending || removeItem.isPending || reorderItems.isPending;

  const moveItem = (index: number, direction: -1 | 1) => {
    const ordered = items.map((item) => item.game.appId);
    const next = moveWithin(ordered, index, index + direction);
    if (next.every((appId, position) => appId === ordered[position])) return;
    reorderItems.mutate(next);
  };

  return (
    <section className="curated-detail" aria-label={`Contenido de ${list.name}`}>
      <header className="curated-detail__head">
        <span
          className="curated-detail__accent"
          style={{ backgroundColor: accentToken(list.accent) }}
          aria-hidden="true"
        />
        <p className="curated-detail__title">
          <strong>{list.name}</strong>
          <data value={detail.data?.total ?? list.gameCount}>
            {gameCountLabel(detail.data?.total ?? list.gameCount)}
          </data>
        </p>
        <p className="curated-detail__meta">
          <span className="curated-detail__kind">{curatedKindLabel(list.kind)}</span>
          {list.description && <span>{list.description}</span>}
        </p>
      </header>

      <div className="curated-detail__add">
        <GamePicker
          label={`Añadir a ${list.name}`}
          placeholder="Nombre del juego"
          disabledAppIds={present}
          disabledHint="Ya está en la lista"
          busyAppId={add.isPending ? add.variables : undefined}
          onPick={(game) => add.mutate(game.appId)}
        />
      </div>

      {message ? (
        <p className="operation-message wishlist-message" role="status">
          {message}
        </p>
      ) : (
        <span className="sr-only" role="status" aria-live="polite" />
      )}

      {detail.isPending ? (
        <ul className="curated-items" aria-busy="true">
          {Array.from({ length: 4 }, (_, slot) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
            <li key={slot} className="curated-item curated-item--skeleton" aria-hidden="true">
              <ShimmerSkeleton aspectRatio="2 / 3" width={34} radiusPx={2} />
              <ShimmerSkeleton height={12} width="55%" radiusPx={2} />
            </li>
          ))}
        </ul>
      ) : detail.isError ? (
        <p className="wishlist-empty" role="alert">
          <strong>No se pudo leer el contenido</strong>
          {getErrorMessage(detail.error)}
          <Button variant="outline" size="sm" onClick={() => void detail.refetch()}>
            Reintentar
          </Button>
        </p>
      ) : !items.length ? (
        <p className="wishlist-empty">
          <strong>La lista está vacía</strong>
          Busca un juego arriba y añádelo. El orden en que los pongas es el argumento de la lista.
        </p>
      ) : (
        <ul className="curated-items">
          {items.map((item, index) => (
            <li
              key={item.game.appId}
              className="curated-item"
              data-highlight={item.highlight}
              data-position={index + 1}
            >
              <span className="curated-item__rank" aria-hidden="true">
                {index + 1}
              </span>
              <span className="curated-item__cover" aria-hidden="true">
                <Artwork
                  appId={item.game.appId}
                  src={item.game.coverUrl}
                  title={item.game.title}
                  kind="cover"
                />
              </span>
              <div className="curated-item__body">
                <p className="curated-item__title">{item.game.title}</p>
                {noteDraft?.appId === item.game.appId ? (
                  <div className="curated-item__editor">
                    <Textarea
                      rows={2}
                      value={noteDraft.note}
                      maxLength={400}
                      aria-label={`Nota de ${item.game.title}`}
                      onChange={(event) =>
                        setNoteDraft({ appId: item.game.appId, note: event.currentTarget.value })
                      }
                    />
                    <Button
                      size="xs"
                      disabled={busy}
                      onClick={() =>
                        update.mutate({
                          appId: item.game.appId,
                          note: noteDraft.note.trim(),
                          highlight: item.highlight,
                        })
                      }
                    >
                      {update.isPending && <IconLoader2 className="is-spinning" />} Guardar nota
                    </Button>
                    <Button variant="ghost" size="xs" onClick={() => setNoteDraft(undefined)}>
                      Cancelar
                    </Button>
                  </div>
                ) : (
                  <button
                    type="button"
                    className="curated-item__note"
                    aria-label={`Editar la nota de ${item.game.title}`}
                    onClick={() => setNoteDraft({ appId: item.game.appId, note: item.note })}
                  >
                    {item.note || "Añadir una nota"}
                  </button>
                )}
              </div>
              <span className="curated-item__actions">
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={
                    item.highlight
                      ? `Quitar destacado a ${item.game.title}`
                      : `Destacar ${item.game.title}`
                  }
                  aria-pressed={item.highlight}
                  disabled={busy}
                  onClick={() =>
                    update.mutate({
                      appId: item.game.appId,
                      note: item.note,
                      highlight: !item.highlight,
                    })
                  }
                >
                  {item.highlight ? <IconStarFilled /> : <IconStar />}
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Subir ${item.game.title}`}
                  disabled={busy || index === 0}
                  onClick={() => moveItem(index, -1)}
                >
                  <IconArrowUp />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Bajar ${item.game.title}`}
                  disabled={busy || index === items.length - 1}
                  onClick={() => moveItem(index, 1)}
                >
                  <IconArrowDown />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Quitar ${item.game.title} de ${list.name}`}
                  disabled={busy}
                  onClick={() => removeItem.mutate(item.game.appId)}
                >
                  <IconX />
                </Button>
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/* --- Editor de la lista -------------------------------------------------- */

function CuratedListEditor({
  open,
  list,
  pending,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  list?: CuratedList | undefined;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: SaveCuratedListInput) => void;
}) {
  const fieldId = useId();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [kind, setKind] = useState<CuratedListKind>("manual");
  const [accent, setAccent] = useState<CuratedAccent>("cyan");
  const [icon, setIcon] = useState("bookmark");
  const [pinned, setPinned] = useState(false);
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    if (!open) return;
    setName(list?.name ?? "");
    setDescription(list?.description ?? "");
    setKind(list?.kind ?? "manual");
    setAccent(list?.accent ?? "cyan");
    setIcon(list?.icon ?? "bookmark");
    setPinned(list?.pinned ?? false);
    setInvalid(false);
  }, [open, list]);

  const submit = () => {
    const clean = name.trim();
    if (!clean) {
      setInvalid(true);
      return;
    }
    onSubmit({
      ...(list ? { id: list.id } : {}),
      name: clean,
      description: description.trim(),
      kind,
      accent,
      icon,
      pinned,
    });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="curated-editor">
        <DialogHeader>
          <DialogTitle>{list ? "Editar lista curada" : "Nueva lista curada"}</DialogTitle>
          <DialogDescription>
            Una selección con criterio propio: el orden y las notas son el contenido, no un adorno.
          </DialogDescription>
        </DialogHeader>
        <div className="curated-editor__fields">
          <label htmlFor={`${fieldId}-name`}>
            <span>Nombre</span>
            <Input
              id={`${fieldId}-name`}
              value={name}
              maxLength={80}
              aria-invalid={invalid}
              placeholder="Ej. Empezar en los metroidvania"
              onChange={(event) => setName(event.currentTarget.value)}
            />
          </label>
          {invalid && (
            <p className="inline-notice" data-kind="error" role="alert">
              <span>La lista necesita un nombre.</span>
            </p>
          )}
          <label htmlFor={`${fieldId}-description`}>
            <span>Descripción</span>
            <Textarea
              id={`${fieldId}-description`}
              rows={2}
              value={description}
              maxLength={400}
              placeholder="Para quién es esta lista y qué defiende"
              onChange={(event) => setDescription(event.currentTarget.value)}
            />
          </label>
          <div className="curated-editor__grid">
            <label htmlFor={`${fieldId}-kind`}>
              <span>Tipo</span>
              <Select value={kind} onValueChange={(value) => setKind(value as CuratedListKind)}>
                <SelectTrigger id={`${fieldId}-kind`} aria-label="Tipo de lista curada">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CURATED_KINDS.map((option) => (
                    <SelectItem key={option.id} value={option.id}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            <label htmlFor={`${fieldId}-accent`}>
              <span>Acento</span>
              <Select value={accent} onValueChange={(value) => setAccent(value as CuratedAccent)}>
                <SelectTrigger id={`${fieldId}-accent`} aria-label="Acento de la lista">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CURATED_ACCENTS.map((option) => (
                    <SelectItem key={option.id} value={option.id}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            <label htmlFor={`${fieldId}-icon`}>
              <span>Icono</span>
              <Select value={icon} onValueChange={setIcon}>
                <SelectTrigger id={`${fieldId}-icon`} aria-label="Icono de la lista">
                  <CollectionIcon name={icon} fallback="manual" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {collectionIconOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      <option.icon aria-hidden="true" /> {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
          </div>
          <div className="curated-editor__switch">
            <span id={`${fieldId}-pinned-label`}>Fijar arriba</span>
            <Switch
              checked={pinned}
              onCheckedChange={setPinned}
              aria-labelledby={`${fieldId}-pinned-label`}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancelar
          </Button>
          <Button onClick={submit} disabled={pending}>
            {pending && <IconLoader2 className="is-spinning" />} Guardar lista
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
