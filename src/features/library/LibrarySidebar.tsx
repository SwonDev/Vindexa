import { useDroppable } from "@dnd-kit/core";
import { SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  IconBook2,
  IconBuildingStore,
  IconChevronDown,
  IconDeviceGamepad2,
  IconFolder,
  IconFolders,
  IconGripVertical,
  IconPlus,
  IconSquareFilled,
  IconUsersGroup,
} from "@tabler/icons-react";
import { useState } from "react";
import { AnimatedNumber, PressableSurface } from "@/components/motion";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  readLibrarySectionExpanded,
  writeLibrarySectionExpanded,
} from "@/features/library/library-session";
import { CollectionContextMenu, StoreContextMenu } from "@/features/library/SidebarContextMenus";
import type { AppBootstrap } from "@/lib/types";
import { collectionDropId, collectionOrderDragId, statusDropId } from "./library-dnd";

/** Tiendas que pueden aparecer como ámbito, con el nombre que se les enseña. */
const EXTERNAL_STORES = [
  { id: "epic", label: "Epic Games", icon: IconBuildingStore },
  { id: "gog", label: "GOG", icon: IconBuildingStore },
  { id: "itch", label: "itch.io", icon: IconBuildingStore },
] as const;

export interface LibraryScope {
  kind: "all" | "installed" | "family" | "store" | "status" | "collection";
  /** Identificador del estado, la colección o la tienda. */
  id?: string;
  label: string;
}

interface LibrarySidebarProps {
  bootstrap?: AppBootstrap | undefined;
  scope: LibraryScope;
  onScopeChange: (scope: LibraryScope) => void;
  onCreateCollection: () => void;
  draggingGames?: boolean | undefined;
  collectionReorderEnabled?: boolean | undefined;
  /** Abre el editor completo de una colección desde su menú rápido. */
  onEditCollection?: ((id: string) => void) | undefined;
  onDeleteCollection?: ((id: string) => void) | undefined;
}

export function LibrarySidebar({
  bootstrap,
  scope,
  onScopeChange,
  onCreateCollection,
  onEditCollection,
  onDeleteCollection,
  draggingGames = false,
  collectionReorderEnabled = false,
}: LibrarySidebarProps) {
  const [statusesExpanded, setStatusesExpanded] = useState(() =>
    readLibrarySectionExpanded("statuses"),
  );
  const selected = (kind: LibraryScope["kind"], id?: string) =>
    scope.kind === kind && scope.id === id;
  return (
    <aside className="library-sidebar" aria-label="Navegación de biblioteca">
      <div className="sidebar-section">
        <p className="sidebar-heading">BIBLIOTECA</p>
        <SidebarItem
          active={selected("all")}
          icon={IconBook2}
          label="Todos los juegos"
          count={bootstrap?.stats.totalGames}
          onClick={() => onScopeChange({ kind: "all", label: "Todos los juegos" })}
        />
        <SidebarItem
          active={selected("installed")}
          icon={IconDeviceGamepad2}
          label="Instalados"
          count={bootstrap?.stats.installedGames}
          onClick={() => onScopeChange({ kind: "installed", label: "Instalados" })}
        />
        <SidebarItem
          active={selected("family")}
          icon={IconUsersGroup}
          label="Steam Family"
          // El recuento sale del arranque, no del listado del propio ámbito.
          // Antes dependía de una consulta que sólo se ejecuta cuando ya estás
          // dentro: había que entrar para ver el número que te dice que hay algo
          // dentro, así que parecía que no había nada.
          count={bootstrap?.stats.familyCatalogGames}
          onClick={() => onScopeChange({ kind: "family", label: "Steam Family" })}
        />
        {/* Cada tienda vinculada es un ámbito más de la biblioteca, no un panel
            de Ajustes: sus juegos se miran igual que los demás. Sólo aparece la
            que tiene algo que enseñar. */}
        {EXTERNAL_STORES.map((store) =>
          (bootstrap?.stats.externalStoreGames?.[store.id] ?? 0) > 0 ? (
            <StoreContextMenu key={store.id} storeId={store.id} storeLabel={store.label}>
              <SidebarItem
                active={scope.kind === "store" && scope.id === store.id}
                icon={store.icon}
                label={store.label}
                count={bootstrap?.stats.externalStoreGames?.[store.id]}
                onClick={() => onScopeChange({ kind: "store", id: store.id, label: store.label })}
              />
            </StoreContextMenu>
          ) : null,
        )}
      </div>
      <div className="sidebar-section">
        <button
          type="button"
          className="sidebar-heading sidebar-heading--toggle"
          aria-controls="library-statuses"
          aria-expanded={statusesExpanded}
          onClick={() => {
            setStatusesExpanded((current) => {
              writeLibrarySectionExpanded("statuses", !current);
              return !current;
            });
          }}
        >
          <span>ESTADOS</span>
          <IconChevronDown size={13} data-expanded={statusesExpanded} />
        </button>
        <div className="sidebar-list" id="library-statuses" hidden={!statusesExpanded}>
          {bootstrap?.statuses.map((status) => (
            <SidebarItem
              key={status.id}
              active={selected("status", status.id)}
              icon={IconSquareFilled}
              iconColor={status.color}
              label={status.name}
              count={status.gameCount}
              dropId={statusDropId(status.id)}
              draggingGames={draggingGames}
              onClick={() => onScopeChange({ kind: "status", id: status.id, label: status.name })}
            />
          ))}
        </div>
      </div>
      <div className="sidebar-section sidebar-section--grow">
        <div className="sidebar-heading-row">
          <span className="sidebar-heading">COLECCIONES</span>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="Crear colección"
                onClick={onCreateCollection}
              >
                <IconPlus />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Crear colección</TooltipContent>
          </Tooltip>
        </div>
        <SortableContext
          items={(bootstrap?.collections ?? []).map((collection) =>
            collectionOrderDragId(collection.id),
          )}
          strategy={verticalListSortingStrategy}
        >
          <div className="sidebar-list">
            {bootstrap?.collections.map((collection) => (
              <CollectionContextMenu
                key={collection.id}
                collection={collection}
                onEdit={() => onEditCollection?.(collection.id)}
                onDelete={() => onDeleteCollection?.(collection.id)}
              >
                <CollectionSidebarItem
                  active={selected("collection", collection.id)}
                  collection={collection}
                  draggingGames={draggingGames}
                  reorderEnabled={collectionReorderEnabled}
                  onClick={() =>
                    onScopeChange({ kind: "collection", id: collection.id, label: collection.name })
                  }
                />
              </CollectionContextMenu>
            ))}
          </div>
        </SortableContext>
      </div>
    </aside>
  );
}

function SidebarItem({
  active,
  icon: ItemIcon,
  iconColor,
  label,
  count,
  onClick,
  dropId,
  dropDisabled = false,
  draggingGames = false,
}: {
  active: boolean;
  icon: typeof IconBook2;
  iconColor?: string | undefined;
  label: string;
  count?: number | undefined;
  onClick: () => void;
  dropId?: string | undefined;
  dropDisabled?: boolean | undefined;
  draggingGames?: boolean | undefined;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: dropId ?? `navigation:${label}`,
    disabled: !dropId,
  });
  const restrictionId = dropDisabled ? `drop-restriction-${dropId}` : undefined;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <PressableSurface asChild>
          <button
            ref={setNodeRef}
            type="button"
            className="sidebar-item"
            data-active={active}
            data-drop-target={Boolean(dropId && !dropDisabled && draggingGames)}
            data-drop-over={isOver && !dropDisabled}
            data-drop-rejected={isOver && dropDisabled}
            data-drop-disabled={Boolean(dropId && dropDisabled && draggingGames)}
            aria-label={label}
            aria-describedby={draggingGames ? restrictionId : undefined}
            onClick={onClick}
          >
            <ItemIcon aria-hidden="true" size={15} style={{ color: iconColor }} />
            <span>{label}</span>
            {typeof count === "number" && (
              /* El recuento se mueve cuando cambia: sincronizar una tienda o
               clasificar un juego se nota aquí sin tener que buscarlo. */
              <data value={count}>
                <AnimatedNumber value={count} />
              </data>
            )}
          </button>
        </PressableSurface>
      </TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
      {restrictionId && (
        <span id={restrictionId} className="sr-only">
          Colección inteligente: no admite juegos soltados; edita sus reglas.
        </span>
      )}
    </Tooltip>
  );
}

function CollectionSidebarItem({
  active,
  collection,
  draggingGames,
  reorderEnabled,
  onClick,
}: {
  active: boolean;
  collection: NonNullable<AppBootstrap["collections"]>[number];
  draggingGames: boolean;
  reorderEnabled: boolean;
  onClick: () => void;
}) {
  const dropDisabled = collection.kind === "smart";
  const dropId = collectionDropId(collection.id);
  const restrictionId = `drop-restriction-${dropId}`;
  const { setNodeRef: setDropNodeRef, isOver } = useDroppable({
    id: dropId,
    disabled: !draggingGames,
  });
  const {
    attributes,
    listeners,
    setNodeRef: setSortNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({
    id: collectionOrderDragId(collection.id),
    disabled: !reorderEnabled || draggingGames,
    data: { type: "collection-order", collectionId: collection.id, title: collection.name },
  });
  return (
    <div
      ref={(node) => {
        setDropNodeRef(node);
        setSortNodeRef(node);
      }}
      className="sidebar-collection"
      data-drop-target={Boolean(!dropDisabled && draggingGames)}
      data-drop-over={isOver && !dropDisabled}
      data-drop-rejected={isOver && dropDisabled}
      data-drop-disabled={Boolean(dropDisabled && draggingGames)}
      data-collection-sorting={isDragging}
      style={{ transform: CSS.Transform.toString(transform), transition }}
    >
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            className="sidebar-item"
            data-active={active}
            aria-label={collection.name}
            aria-describedby={draggingGames && dropDisabled ? restrictionId : undefined}
            onClick={onClick}
          >
            {collection.kind === "smart" ? (
              <IconFolders aria-hidden="true" size={15} style={{ color: collection.color }} />
            ) : (
              <IconFolder aria-hidden="true" size={15} style={{ color: collection.color }} />
            )}
            <span>{collection.name}</span>
            <data value={collection.gameCount}>{collection.gameCount.toLocaleString("es-ES")}</data>
          </button>
        </TooltipTrigger>
        <TooltipContent side="right">{collection.name}</TooltipContent>
      </Tooltip>
      <button
        ref={setActivatorNodeRef}
        type="button"
        className="sidebar-collection__handle"
        aria-label={`Reordenar colección ${collection.name}`}
        title="Arrastra o pulsa Espacio y usa las flechas"
        {...attributes}
        {...listeners}
      >
        <IconGripVertical aria-hidden="true" />
      </button>
      {dropDisabled && (
        <span id={restrictionId} className="sr-only">
          Colección inteligente: no admite juegos soltados; edita sus reglas.
        </span>
      )}
    </div>
  );
}
