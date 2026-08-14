import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
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
  IconFolders,
  IconGripVertical,
  IconLoader2,
  IconPencil,
  IconPlus,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { EmptyState } from "@/components/common/EmptyState";
import { LoadingState } from "@/components/common/LoadingState";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CollectionEditorDialog } from "@/features/collections/CollectionEditorDialog";
import { CollectionIcon } from "@/features/collections/CollectionIcon";
import {
  collectionOrderDragId,
  parseCollectionOrderDragId,
  reorderCollectionIds,
} from "@/features/library/library-dnd";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppBootstrap, CollectionSummary } from "@/lib/types";

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
  const [editorOpen, setEditorOpen] = useState(false);
  const [message, setMessage] = useState<string>();

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

  const persistOrder = (next: string[]) => {
    const previous = collections.map((collection) => collection.id);
    if (next === previous || next.every((id, index) => id === previous[index])) return;
    setCollectionOrder(next);
    reorder.mutate({ previous, next });
  };
  const moveCollection = (id: string, direction: -1 | 1) => {
    const previous = collections.map((collection) => collection.id);
    const current = previous.indexOf(id);
    const target = current + direction;
    if (current < 0 || target < 0 || target >= previous.length) return;
    persistOrder(reorderCollectionIds(previous, id, previous[target] ?? id));
  };
  const onDragEnd = ({ active, over }: DragEndEvent) => {
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
  const openCreate = () => {
    setEditorCollection(undefined);
    setEditorOpen(true);
  };
  const openEdit = (collection: CollectionSummary) => {
    setEditorCollection(collection);
    setEditorOpen(true);
  };

  if (loading && !bootstrap) return <LoadingState label="Cargando colecciones" />;
  return (
    <section className="collections-screen">
      <header className="screen-heading">
        <div>
          <p className="eyebrow">ORGANIZACIÓN TRANSVERSAL</p>
          <h1>Colecciones</h1>
          <p>
            Agrupa juegos manualmente o deja que reglas verificables mantengan la selección al día.
          </p>
        </div>
        <Button onClick={openCreate}>
          <IconPlus /> Nueva colección
        </Button>
      </header>
      {message && (
        <p className="operation-message collections-message" role="status">
          {reorder.isPending ? "Guardando orden…" : message}
        </p>
      )}
      {!collections.length ? (
        <EmptyState
          icon={IconFolders}
          title="Todavía no hay colecciones"
          description="Crea una colección manual o una inteligente con vista previa de sus reglas."
          action={
            <Button onClick={openCreate}>
              <IconPlus /> Crear la primera
            </Button>
          }
        />
      ) : (
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
          <SortableContext
            items={collections.map((collection) => collectionOrderDragId(collection.id))}
            strategy={rectSortingStrategy}
          >
            <div className="collections-grid">
              {collections.map((collection, index) => (
                <CollectionCard
                  key={collection.id}
                  collection={collection}
                  index={index}
                  total={collections.length}
                  busy={deletion.isPending || reorder.isPending}
                  onEdit={() => openEdit(collection)}
                  onMove={(direction) => moveCollection(collection.id, direction)}
                  onDelete={() => deletion.mutate(collection.id)}
                />
              ))}
            </div>
          </SortableContext>
        </DndContext>
      )}
      <CollectionEditorDialog
        open={editorOpen}
        onOpenChange={(next) => {
          setEditorOpen(next);
          if (!next) setEditorCollection(undefined);
        }}
        collection={editorCollection}
        statuses={bootstrap?.statuses}
      />
    </section>
  );
}

function CollectionCard({
  collection,
  index,
  total,
  busy,
  onEdit,
  onMove,
  onDelete,
}: {
  collection: CollectionSummary;
  index: number;
  total: number;
  busy: boolean;
  onEdit: () => void;
  onMove: (direction: -1 | 1) => void;
  onDelete: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: collectionOrderDragId(collection.id),
    data: { title: collection.name },
  });
  return (
    <article
      ref={setNodeRef}
      className="collection-card"
      data-dragging={isDragging}
      style={{ transform: CSS.Transform.toString(transform), transition }}
    >
      <div className="collection-card__stripe" style={{ backgroundColor: collection.color }} />
      <div className="collection-card__header">
        <span className="collection-card__icon" style={{ color: collection.color }}>
          <CollectionIcon name={collection.icon} fallback={collection.kind} />
        </span>
        <div>
          <h2>{collection.name}</h2>
          <span>{collection.kind === "smart" ? "Inteligente" : "Manual"}</span>
        </div>
        <data>{collection.gameCount.toLocaleString("es-ES")}</data>
      </div>
      <p>{collection.description || "Sin descripción"}</p>
      <div className="collection-card__footer">
        {collection.kind === "smart" ? (
          <Badge variant="outline">
            {collection.matchMode === "all" ? "Todas las reglas" : "Cualquier regla"}
          </Badge>
        ) : (
          <Badge variant="secondary">Orden manual</Badge>
        )}
        <div className="collection-card__actions">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Arrastrar ${collection.name}`}
            disabled={busy}
            {...attributes}
            {...listeners}
          >
            <IconGripVertical />
          </Button>
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
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="ghost" size="icon-sm" aria-label={`Eliminar ${collection.name}`}>
                <IconTrash />
              </Button>
            </AlertDialogTrigger>
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
        </div>
      </div>
    </article>
  );
}
