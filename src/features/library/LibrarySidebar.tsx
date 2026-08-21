import { useDroppable } from "@dnd-kit/core";
import { SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  IconBook2,
  IconBuildingStore,
  IconChevronDown,
  IconDeviceGamepad2,
  IconGripVertical,
  IconPlus,
  IconShieldCheck,
  IconSquareFilled,
  IconUsersGroup,
} from "@tabler/icons-react";
import { type ComponentPropsWithoutRef, forwardRef, useCallback, useState } from "react";
import { AnimatedNumber, PressableSurface, StaggerList } from "@/components/motion";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { CollectionIcon } from "@/features/collections/CollectionIcon";
import {
  readLibrarySectionExpanded,
  writeLibrarySectionExpanded,
} from "@/features/library/library-session";
import {
  CollectionContextMenu,
  type SidebarNotice,
  StatusContextMenu,
  StoreContextMenu,
} from "@/features/library/SidebarContextMenus";
import { useToast } from "@/features/shell/toasts";
import { storeShortLabel } from "@/lib/stores";
import type { AppBootstrap } from "@/lib/types";
import { collectionDropId, collectionOrderDragId, statusDropId } from "./library-dnd";

/**
 * Identidad del aviso de sincronización.
 *
 * Es fija a propósito: así «Sincronizando Epic Games Store…» se convierte en su
 * resultado en el mismo sitio, en vez de apilar dos avisos que dicen lo mismo.
 */
const SYNC_TOAST = "sidebar-store-sync";

/** Un aviso de trabajo en marcha no se cierra solo: todavía no hay resultado. */
function pendingToast(message: string) {
  return { message, kind: "info" as const, autoDismissMs: 0 };
}

/** Tiendas que pueden aparecer como ámbito, con el nombre que se les enseña. */
const EXTERNAL_STORES = [
  { id: "epic", label: storeShortLabel("epic"), icon: IconBuildingStore },
  { id: "gog", label: storeShortLabel("gog"), icon: IconBuildingStore },
  { id: "itch", label: storeShortLabel("itch"), icon: IconBuildingStore },
] as const;

export interface LibraryScope {
  kind: "all" | "installed" | "drmFree" | "family" | "store" | "status" | "collection";
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
  /** Abre el editor de estados desde el menú rápido de uno de ellos. */
  onEditStatuses?: (() => void) | undefined;
}

export function LibrarySidebar({
  bootstrap,
  scope,
  onScopeChange,
  onCreateCollection,
  onEditCollection,
  onDeleteCollection,
  onEditStatuses,
  draggingGames = false,
  collectionReorderEnabled = false,
}: LibrarySidebarProps) {
  const [statusesExpanded, setStatusesExpanded] = useState(() =>
    readLibrarySectionExpanded("statuses"),
  );
  // Un menú contextual se cierra al elegir, así que lo que tarda —sincronizar
  // una tienda— se cuenta en la pila de avisos de la aplicación, que sabe
  // sustituir «sincronizando…» por su resultado y no cierra un error solo.
  const toast = useToast();
  const notify = useCallback(
    (notice: SidebarNotice | undefined) => {
      if (!notice) return;
      if (notice.kind === "pending")
        toast.show({ id: SYNC_TOAST, ...pendingToast(notice.message) });
      else toast.show({ id: SYNC_TOAST, message: notice.message, kind: notice.kind });
    },
    [toast],
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
        {/* Sin DRM sólo aparece cuando hay algo que enseñar: una entrada con
            cero al lado no es un acceso directo, es una pregunta sin responder.
            Lo que no se ha comprobado no cuenta: `unknown` no es «lleva DRM». */}
        {(bootstrap?.stats.drmFreeGames ?? 0) > 0 && (
          <SidebarItem
            active={selected("drmFree")}
            icon={IconShieldCheck}
            label="Sin DRM"
            count={bootstrap?.stats.drmFreeGames}
            onClick={() => onScopeChange({ kind: "drmFree", label: "Sin DRM" })}
          />
        )}
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
            <StoreContextMenu
              key={store.id}
              storeId={store.id}
              storeLabel={store.label}
              onNotice={notify}
            >
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
        {/* La cabecera también responde al clic derecho: es donde se busca
            «editar estados» cuando se está mirando la lista de estados. */}
        <ContextMenu>
          <ContextMenuTrigger asChild>
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
          </ContextMenuTrigger>
          <ContextMenuContent aria-label="Acciones rápidas de los estados">
            <ContextMenuLabel>Estados</ContextMenuLabel>
            <ContextMenuSeparator />
            <ContextMenuItem
              onSelect={() => {
                setStatusesExpanded((current) => {
                  writeLibrarySectionExpanded("statuses", !current);
                  return !current;
                });
              }}
            >
              {statusesExpanded ? "Plegar la lista" : "Desplegar la lista"}
            </ContextMenuItem>
            {onEditStatuses && (
              <ContextMenuItem onSelect={onEditStatuses}>Editar estados…</ContextMenuItem>
            )}
          </ContextMenuContent>
        </ContextMenu>
        {/* Los estados aparecen escalonados al desplegar la sección. No es
            adorno: el escalonado es lo que hace legible que se ha abierto una
            lista, en vez de que catorce filas salgan de golpe. No se aplica a
            las colecciones porque ésas se arrastran, y dos transformaciones
            sobre el mismo elemento se pelean. */}
        <StaggerList
          as="div"
          className="sidebar-list"
          id="library-statuses"
          hidden={!statusesExpanded}
          itemAsChild
        >
          {bootstrap?.statuses.map((status) => (
            <StatusContextMenu key={status.id} status={status} onEdit={onEditStatuses}>
              <SidebarItem
                active={selected("status", status.id)}
                icon={IconSquareFilled}
                iconColor={status.color}
                label={status.name}
                count={status.gameCount}
                dropId={statusDropId(status.id)}
                draggingGames={draggingGames}
                onClick={() => onScopeChange({ kind: "status", id: status.id, label: status.name })}
              />
            </StatusContextMenu>
          ))}
        </StaggerList>
      </div>
      <div className="sidebar-section sidebar-section--grow">
        <ContextMenu>
          <ContextMenuTrigger asChild>
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
          </ContextMenuTrigger>
          <ContextMenuContent aria-label="Acciones rápidas de las colecciones">
            <ContextMenuLabel>Colecciones</ContextMenuLabel>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={onCreateCollection}>Nueva colección…</ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
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

/**
 * Una entrada de la barra lateral.
 *
 * Reenvía al `<button>` el `ref` y **todo lo que no reconoce**, y eso no es un
 * detalle de estilo: `ContextMenuTrigger asChild` clona a su hijo pasándole
 * `onContextMenu` y un `ref`. Sin reenviarlos, el manejador se quedaba en las
 * props de este componente y no llegaba nunca al elemento del DOM: el clic
 * derecho no abría nada en las tiendas, ni en los estados, ni en las
 * colecciones de la barra lateral, y no fallaba ni una prueba porque el menú
 * estaba escrito y montado.
 *
 * Es el mismo fallo que dejó a media aplicación sin menú contextual, sólo que
 * un piso más abajo: allí lo cancelaba una guarda global; aquí, un componente
 * que se comía las props.
 */
const SidebarItem = forwardRef<
  HTMLButtonElement,
  {
    active: boolean;
    icon: typeof IconBook2;
    iconColor?: string | undefined;
    label: string;
    count?: number | undefined;
    onClick: () => void;
    dropId?: string | undefined;
    dropDisabled?: boolean | undefined;
    draggingGames?: boolean | undefined;
  } & Omit<ComponentPropsWithoutRef<"button">, "onClick" | "className" | "type">
>(function SidebarItem(
  {
    active,
    icon: ItemIcon,
    iconColor,
    label,
    count,
    onClick,
    dropId,
    dropDisabled = false,
    draggingGames = false,
    ...rest
  },
  ref,
) {
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
            // Dos dueños del mismo nodo: dnd-kit lo necesita para saber dónde
            // se suelta, y quien envuelva esto —el menú contextual— para saber
            // sobre qué se ha pulsado.
            ref={(node) => {
              setNodeRef(node);
              if (typeof ref === "function") ref(node);
              else if (ref) ref.current = node;
            }}
            type="button"
            className="sidebar-item"
            data-active={active}
            data-drop-target={Boolean(dropId && !dropDisabled && draggingGames)}
            data-drop-over={isOver && !dropDisabled}
            data-drop-rejected={isOver && dropDisabled}
            data-drop-disabled={Boolean(dropId && dropDisabled && draggingGames)}
            aria-label={label}
            aria-describedby={draggingGames ? restrictionId : undefined}
            {...rest}
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
});

/**
 * Una colección en la barra lateral.
 *
 * Como [`SidebarItem`], reenvía el `ref` y lo que no reconoce: el envoltorio es
 * el menú contextual, y sin esto su `onContextMenu` no llegaba al DOM. Aquí el
 * destino es el contenedor entero —no sólo el botón— para que el clic derecho
 * responda también sobre el asa de arrastre.
 */
const CollectionSidebarItem = forwardRef<
  HTMLDivElement,
  {
    active: boolean;
    collection: NonNullable<AppBootstrap["collections"]>[number];
    draggingGames: boolean;
    reorderEnabled: boolean;
    onClick: () => void;
  } & Omit<ComponentPropsWithoutRef<"div">, "onClick" | "className" | "style">
>(function CollectionSidebarItem(
  { active, collection, draggingGames, reorderEnabled, onClick, ...rest },
  ref,
) {
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
        if (typeof ref === "function") ref(node);
        else if (ref) ref.current = node;
      }}
      {...rest}
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
            {/* El icono sale de la colección, no del tipo: si se elige uno en
                el menú rápido o en el editor, aquí es donde hay que verlo. */}
            <CollectionIcon
              name={collection.icon}
              fallback={collection.kind === "smart" ? "smart" : "manual"}
              size={15}
              style={{ color: collection.color }}
            />
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
});
