import {
  IconArrowsExchange,
  IconCopy,
  IconGauge,
  IconInfoCircle,
  IconPalette,
  IconPencil,
  IconSettings,
  IconTrash,
} from "@tabler/icons-react";
import { type ReactNode, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { QUICK_COLLECTION_COLORS } from "@/features/library/SidebarContextMenus";
import { PlannerItemEditor } from "@/features/planner/PlannerViews";
import type { PlannerColumn, PlannerItem, SavePlannerItemInput } from "@/lib/types";

/**
 * Acciones rápidas de una tarjeta del planificador.
 *
 * # Por qué las de aquí no son las de la biblioteca
 *
 * Una tarjeta del planificador no es un juego cualquiera: es un juego **con un
 * plan**. Los verbos que tienen sentido son los del plan —moverlo de columna,
 * cambiar su objetivo, sacarlo— y no el estado ni la prioridad, que ya viven en
 * su ficha. Por eso este menú es propio y no una copia del de la biblioteca.
 *
 * # Quitar del planificador
 *
 * `remove_planner_item` existía en el backend, viajaba por el puente y no había
 * forma de llamarlo desde ninguna pantalla: para sacar un juego del plan había
 * que arrastrarlo fuera, y eso no lo saca, lo mueve. Aquí se pregunta antes,
 * porque al salir se pierden el objetivo y la fecha escritos a mano.
 */
export function PlannerCardContextMenu({
  item,
  columns,
  currentColumnId,
  children,
  busy = false,
  onMoveToColumn,
  onRemove,
  onOpenDetail,
  onSaveItem,
}: {
  item: PlannerItem;
  columns: readonly PlannerColumn[];
  currentColumnId?: string | undefined;
  children: ReactNode;
  busy?: boolean | undefined;
  onMoveToColumn: (columnId: string) => void;
  onRemove: () => void;
  onOpenDetail: (appId: number) => void;
  onSaveItem: (input: SavePlannerItemInput) => Promise<void>;
}) {
  const [editando, setEditando] = useState(false);
  const [confirmandoSalida, setConfirmandoSalida] = useState(false);
  const otras = columns.filter((column) => column.id !== currentColumnId);

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
        <ContextMenuContent aria-label={`Acciones rápidas de ${item.title}`}>
          <ContextMenuLabel>{item.title}</ContextMenuLabel>
          <ContextMenuSeparator />

          <ContextMenuItem onSelect={() => onOpenDetail(item.appId)}>
            <IconInfoCircle aria-hidden="true" /> Abrir ficha
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => setEditando(true)}>
            <IconPencil aria-hidden="true" /> Editar planificación…
          </ContextMenuItem>

          {otras.length > 0 && (
            <>
              <ContextMenuSeparator />
              <ContextMenuSub>
                <ContextMenuSubTrigger>
                  <IconArrowsExchange aria-hidden="true" /> Mover a
                </ContextMenuSubTrigger>
                <ContextMenuSubContent>
                  {otras.map((column) => (
                    <ContextMenuItem
                      key={column.id}
                      disabled={busy}
                      onSelect={() => onMoveToColumn(column.id)}
                    >
                      <span
                        aria-hidden="true"
                        className="size-2 shrink-0 rounded-[1px]"
                        style={{ backgroundColor: column.color }}
                      />
                      {column.name}
                    </ContextMenuItem>
                  ))}
                </ContextMenuSubContent>
              </ContextMenuSub>
            </>
          )}

          <ContextMenuSeparator />
          <ContextMenuItem
            onSelect={() => {
              navigator.clipboard?.writeText(item.title).catch(() => undefined);
            }}
          >
            <IconCopy aria-hidden="true" /> Copiar título
          </ContextMenuItem>

          <ContextMenuSeparator />
          <ContextMenuItem
            variant="destructive"
            disabled={busy}
            onSelect={() => setConfirmandoSalida(true)}
          >
            <IconTrash aria-hidden="true" /> Quitar del planificador
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>

      {/* Montado sin botón: lo abre el menú de arriba. */}
      <PlannerItemEditor
        item={item}
        onSave={onSaveItem}
        open={editando}
        onOpenChange={setEditando}
      />

      <AlertDialog open={confirmandoSalida} onOpenChange={setConfirmandoSalida}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>¿Sacar “{item.title}” del planificador?</AlertDialogTitle>
            <AlertDialogDescription>
              El juego sigue en tu biblioteca con su estado y su progreso. Lo que se pierde es lo
              escrito aquí: el objetivo, la fecha y la estimación de tiempo.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancelar</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmandoSalida(false);
                onRemove();
              }}
            >
              Quitar del plan
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

/** Límites de trabajo que se ofrecen sin abrir nada. */
const LIMITES = [0, 1, 2, 3, 4, 5, 6, 8, 10] as const;

/**
 * Acciones rápidas de una columna del planificador.
 *
 * El color y el límite de trabajo de una columna vivían sólo en Ajustes →
 * Organización, a tres clics de la pantalla donde se miran. Son los dos ajustes
 * que se tocan mientras se planifica, así que se tocan aquí; lo que cambia el
 * significado de la columna —renombrarla, borrarla, reordenarla— sigue en
 * Ajustes, porque afecta a todo lo que hay dentro.
 */
export function PlannerLaneContextMenu({
  column,
  children,
  busy = false,
  onChangeColor,
  onChangeLimit,
  onOpenSettings,
}: {
  column: PlannerColumn;
  children: ReactNode;
  busy?: boolean | undefined;
  onChangeColor: (color: string) => void;
  onChangeLimit: (limit: number | undefined) => void;
  onOpenSettings?: (() => void) | undefined;
}) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${column.name}`}>
        <ContextMenuLabel>{column.name}</ContextMenuLabel>
        <ContextMenuSeparator />

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <IconPalette aria-hidden="true" /> Color
          </ContextMenuSubTrigger>
          <ContextMenuSubContent className="sidebar-picker sidebar-picker--colors">
            {QUICK_COLLECTION_COLORS.map((option) => (
              <ContextMenuItem
                key={option.value}
                aria-label={option.label}
                title={option.label}
                disabled={busy}
                data-selected={column.color.toLowerCase() === option.value.toLowerCase()}
                onSelect={() => onChangeColor(option.value)}
              >
                <span
                  className="sidebar-swatch"
                  style={{ background: option.value }}
                  aria-hidden="true"
                />
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <IconGauge aria-hidden="true" /> Límite de trabajo
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuRadioGroup
              value={String(column.wipLimit ?? 0)}
              onValueChange={(value) => onChangeLimit(Number(value) || undefined)}
            >
              {LIMITES.map((limite) => (
                <ContextMenuRadioItem key={limite} value={String(limite)} disabled={busy}>
                  {limite === 0 ? "Sin límite" : `${limite} juegos`}
                </ContextMenuRadioItem>
              ))}
            </ContextMenuRadioGroup>
          </ContextMenuSubContent>
        </ContextMenuSub>

        {onOpenSettings && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={onOpenSettings}>
              <IconSettings aria-hidden="true" /> Editar columnas…
            </ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}
