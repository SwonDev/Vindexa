import { useDroppable } from "@dnd-kit/core";
import { SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  IconBook2,
  IconChevronDown,
  IconCircleFilled,
  IconDeviceGamepad2,
  IconFolder,
  IconFolders,
  IconGripVertical,
  IconPlus,
  IconUsersGroup,
} from "@tabler/icons-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  readLibrarySectionExpanded,
  writeLibrarySectionExpanded,
} from "@/features/library/library-session";
import type { AppBootstrap } from "@/lib/types";
import { collectionDropId, collectionOrderDragId, statusDropId } from "./library-dnd";

export interface LibraryScope {
  kind: "all" | "installed" | "family" | "status" | "collection";
  id?: string;
  label: string;
}

interface LibrarySidebarProps {
  bootstrap?: AppBootstrap | undefined;
  scope: LibraryScope;
  onScopeChange: (scope: LibraryScope) => void;
  onCreateCollection: () => void;
  familyCount?: number | undefined;
  draggingGames?: boolean | undefined;
  collectionReorderEnabled?: boolean | undefined;
}

export function LibrarySidebar({
  bootstrap,
  scope,
  onScopeChange,
  onCreateCollection,
  familyCount,
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
          count={familyCount}
          onClick={() => onScopeChange({ kind: "family", label: "Steam Family" })}
        />
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
              icon={IconCircleFilled}
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
              <CollectionSidebarItem
                key={collection.id}
                active={selected("collection", collection.id)}
                collection={collection}
                draggingGames={draggingGames}
                reorderEnabled={collectionReorderEnabled}
                onClick={() =>
                  onScopeChange({ kind: "collection", id: collection.id, label: collection.name })
                }
              />
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
        <button
          ref={setNodeRef}
          type="button"
          className="sidebar-item"
          data-active={active}
          data-drop-target={Boolean(dropId && !dropDisabled && draggingGames)}
          data-drop-over={isOver && !dropDisabled}
          data-drop-rejected={isOver && dropDisabled}
          data-drop-disabled={Boolean(dropId && dropDisabled && draggingGames)}
          aria-describedby={draggingGames ? restrictionId : undefined}
          onClick={onClick}
        >
          <ItemIcon aria-hidden="true" size={15} style={{ color: iconColor }} />
          <span>{label}</span>
          {typeof count === "number" && <data value={count}>{count.toLocaleString("es-ES")}</data>}
        </button>
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
