import {
  type Announcements,
  closestCenter,
  DndContext,
  type DragEndEvent,
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
  IconAlertTriangle,
  IconArrowBackUp,
  IconCalendar,
  IconChevronLeft,
  IconChevronRight,
  IconLayoutKanban,
  IconPlus,
  IconRefresh,
  IconTargetArrow,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { EmptyState } from "@/components/common/EmptyState";
import { GamePreviewCard } from "@/components/common/GamePreviewCard";
import { LoadingState } from "@/components/common/LoadingState";
import { MetricStrip } from "@/components/common/MetricStrip";
import { PageHeader } from "@/components/common/PageHeader";
import { ProgressMeter } from "@/components/common/ProgressMeter";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  AppBootstrap,
  PlannerColumn,
  PlannerItem,
  PlannerSettings,
  SavePlannerItemInput,
} from "@/lib/types";
import { PlannerCardContextMenu, PlannerLaneContextMenu } from "./PlannerContextMenu";
import {
  PlannerCapacityEditor,
  PlannerItemEditor,
  PlannerPeriodView,
  PlannerQueueView,
  type PlannerViewItem,
} from "./PlannerViews";
import { buildMonthSegments, buildPlannerMetrics, buildWeekSegments } from "./planner-periods";
import "./planner-advanced.css";

/* La ficha llega sólo cuando alguien la pide: el planificador se abre sin ella
   y pesa lo mismo que antes. */
const GameDetailSheet = lazy(() =>
  import("@/features/library/GameDetailSheet").then((module) => ({
    default: module.GameDetailSheet,
  })),
);

type PlannerMode = "kanban" | "queue" | "week" | "month";

const DEFAULT_PLANNER_SETTINGS: PlannerSettings = {
  weeklyCapacityMinutes: 600,
  monthlyCapacityMinutes: 2_400,
};

function localIsoDate(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function shiftAnchor(anchorIso: string, mode: PlannerMode, direction: -1 | 1): string {
  const [yearRaw, monthRaw, dayRaw] = anchorIso.split("-").map(Number);
  const year = yearRaw ?? new Date().getFullYear();
  const month = monthRaw ?? 1;
  const day = dayRaw ?? 1;
  const date = new Date(year, month - 1, day);
  if (mode === "month") date.setMonth(date.getMonth() + direction, 1);
  else date.setDate(date.getDate() + direction * 7);
  return localIsoDate(date);
}

export function PlannerScreen({
  bootstrap,
  loading,
  onOpenSettings,
}: {
  bootstrap?: AppBootstrap;
  loading: boolean;
  onOpenSettings?: ((target: "organization") => void) | undefined;
}) {
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<PlannerMode>("kanban");
  const [anchorIso, setAnchorIso] = useState(localIsoDate);
  const overview = useQuery({
    queryKey: ["planner-overview"],
    queryFn: api.getPlannerOverview,
  });
  const sourceColumns = useMemo(
    () => overview.data?.columns ?? bootstrap?.planner ?? [],
    [bootstrap?.planner, overview.data?.columns],
  );
  const [columns, setColumns] = useState<PlannerColumn[]>(sourceColumns);
  const [activeItem, setActiveItem] = useState<PlannerItem>();
  const [message, setMessage] = useState<string>();
  const [lastMove, setLastMove] = useState<{
    appId: number;
    columnId: string;
    position: number;
    title: string;
  }>();
  useEffect(() => {
    setColumns(sourceColumns);
  }, [sourceColumns]);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 7 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const mutation = useMutation({
    mutationFn: ({
      appId,
      columnId,
      position,
    }: {
      appId: number;
      columnId: string;
      position: number;
    }) => api.movePlannerItem(appId, columnId, position),
    onSuccess: () => {
      setMessage("Plan actualizado");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (error) => {
      setMessage(getErrorMessage(error));
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
  });
  const queueMutation = useMutation({
    mutationFn: ({ appId, position }: { appId: number; position: number }) =>
      api.movePlannerQueueItem(appId, position),
    onSuccess: () => {
      setMessage("Cola actualizada");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  const itemMutation = useMutation({
    mutationFn: (input: SavePlannerItemInput) => api.savePlannerItem(input),
    onSuccess: () => {
      setMessage("Objetivo guardado");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  /* Sacar un juego del plan no existía en ninguna pantalla: `remove_planner_item`
     estaba escrito, expuesto y sin forma de llamarlo. */
  const removeMutation = useMutation({
    mutationFn: (appId: number) => api.removePlannerItem(appId),
    onSuccess: () => {
      setMessage("Juego retirado del planificador");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  const columnMutation = useMutation({
    mutationFn: ({
      column,
      color,
      wipLimit,
    }: {
      column: PlannerColumn;
      color?: string;
      wipLimit?: number | undefined;
    }) =>
      api.savePlannerColumn(
        column.id,
        column.name,
        color ?? column.color,
        wipLimit === undefined && color !== undefined ? column.wipLimit : wipLimit,
      ),
    onSuccess: () => {
      setMessage("Columna actualizada");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  const [openGameId, setOpenGameId] = useState<number>();

  const capacityMutation = useMutation({
    mutationFn: (settings: PlannerSettings) => api.savePlannerCapacity(settings),
    onSuccess: () => {
      setMessage("Capacidad guardada");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  const allItems = useMemo(() => columns.flatMap((column) => column.items), [columns]);
  const settings = overview.data?.settings ?? DEFAULT_PLANNER_SETTINGS;
  const queue = useMemo(
    () =>
      (overview.data?.queue ?? allItems)
        .slice()
        .sort((a, b) => a.queuePosition - b.queuePosition || a.appId - b.appId),
    [allItems, overview.data?.queue],
  );
  const metrics = useMemo(
    () => buildPlannerMetrics(columns, settings, localIsoDate()),
    [columns, settings],
  );
  const period = useMemo(
    () =>
      mode === "month"
        ? buildMonthSegments(columns, anchorIso)
        : buildWeekSegments(columns, anchorIso),
    [anchorIso, columns, mode],
  );
  const plannedIds = useMemo(() => new Set(allItems.map((item) => item.appId)), [allItems]);
  // El destino por defecto del botón principal es la primera columna: es la
  // más inmediata del tablero y la que la persona mira al entrar.
  const firstColumn = columns[0];
  const planSummaryDetail = `${metrics.totalGames} ${
    metrics.totalGames === 1 ? "juego en el plan" : "juegos en el plan"
  } · ${formatPlaytime(metrics.remainingMinutes)} restantes en total · ${metrics.averageProgress} % de progreso medio`;
  const saveItem = async (input: SavePlannerItemInput) => itemMutation.mutateAsync(input);
  const saveCapacity = async (next: PlannerSettings) => {
    await capacityMutation.mutateAsync(next);
  };
  const isSaving =
    mutation.isPending ||
    queueMutation.isPending ||
    itemMutation.isPending ||
    capacityMutation.isPending;

  const findItemColumn = (id: unknown) =>
    columns.find((column) => column.items.some((item) => String(item.appId) === String(id)));
  const plannerInstructions: ScreenReaderInstructions = {
    draggable:
      "Para mover la tarjeta, pulsa espacio o intro para levantarla, usa las flechas para cambiar de columna o de posición y vuelve a pulsar espacio o intro para soltarla. Escape cancela.",
  };
  const plannerAnnouncements: Announcements = {
    onDragStart({ active }) {
      const item = allItems.find((entry) => String(entry.appId) === String(active.id));
      const column = findItemColumn(active.id);
      return item ? `Levantado ${item.title}${column ? ` desde ${column.name}` : ""}.` : undefined;
    },
    onDragOver({ active, over }) {
      if (!over) return undefined;
      const item = allItems.find((entry) => String(entry.appId) === String(active.id));
      const overColumn =
        columns.find((column) => String(column.id) === String(over.id)) ?? findItemColumn(over.id);
      return item && overColumn ? `${item.title} sobre la columna ${overColumn.name}.` : undefined;
    },
    onDragEnd({ active, over }) {
      const item = allItems.find((entry) => String(entry.appId) === String(active.id));
      if (!item) return undefined;
      if (!over) return `Movimiento de ${item.title} cancelado.`;
      const overColumn =
        columns.find((column) => String(column.id) === String(over.id)) ?? findItemColumn(over.id);
      return `${item.title} soltado en ${overColumn?.name ?? "el plan"}.`;
    },
    onDragCancel() {
      return "Movimiento cancelado; la tarjeta vuelve a su sitio.";
    },
  };
  const applyOptimisticMove = (
    appId: number,
    destinationId: string,
    position: number,
    movingItem: PlannerItem,
  ) => {
    setColumns((current) =>
      current.map((column) => ({
        ...column,
        items:
          column.id === destinationId
            ? [
                ...column.items.filter((item) => item.appId !== appId).slice(0, position),
                movingItem,
                ...column.items.filter((item) => item.appId !== appId).slice(position),
              ].filter(Boolean)
            : column.items.filter((item) => item.appId !== appId),
      })),
    );
  };
  const undoLastMove = () => {
    if (!lastMove) return;
    const item = allItems.find((entry) => entry.appId === lastMove.appId);
    if (item) applyOptimisticMove(lastMove.appId, lastMove.columnId, lastMove.position, item);
    mutation.mutate({
      appId: lastMove.appId,
      columnId: lastMove.columnId,
      position: lastMove.position,
    });
    setLastMove(undefined);
  };
  const onDragStart = ({ active }: DragStartEvent) =>
    setActiveItem(allItems.find((item) => String(item.appId) === String(active.id)));
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveItem(undefined);
    if (!over) return;
    const appId = Number(active.id);
    const destination = columns.find(
      (column) =>
        column.id === over.id ||
        column.items.some((item) => String(item.appId) === String(over.id)),
    );
    if (!destination) return;
    const targetIndex = destination.items.findIndex(
      (item) => String(item.appId) === String(over.id),
    );
    const position = targetIndex >= 0 ? targetIndex : destination.items.length;
    const movingItem = activeItem ?? allItems.find((item) => item.appId === appId);
    if (!movingItem) return;
    const originColumn = findItemColumn(appId);
    const originPosition = originColumn
      ? originColumn.items.findIndex((item) => item.appId === appId)
      : 0;
    applyOptimisticMove(appId, destination.id, position, movingItem);
    mutation.mutate(
      { appId, columnId: destination.id, position },
      {
        onSuccess: () =>
          setLastMove(
            originColumn
              ? {
                  appId,
                  columnId: originColumn.id,
                  position: Math.max(0, originPosition),
                  title: movingItem.title,
                }
              : undefined,
          ),
        onError: () => setLastMove(undefined),
      },
    );
  };

  if ((loading || overview.isPending) && !bootstrap)
    return <LoadingState label="Cargando planificador" />;
  if (overview.isError && !bootstrap) {
    return (
      <section className="planner-screen">
        <div className="planner-load-error" role="alert">
          <IconAlertTriangle />
          <strong>No se pudo abrir el planificador</strong>
          <span className="planner-load-error__copy">{getErrorMessage(overview.error)}</span>
          <Button
            variant="outline"
            onClick={() => void overview.refetch()}
            aria-label="Reintentar planificador"
          >
            <IconRefresh /> Reintentar
          </Button>
        </div>
      </section>
    );
  }
  return (
    <section className="planner-screen">
      <PageHeader
        eyebrow="PLAN DE JUEGO"
        title="Planificador"
        actions={
          <>
            {/* Una sola cifra en el encabezado: la que decide si el jueves cabe
                otra partida. El resto del agregado —cuántos juegos hay en plan
                y cuánto queda en total— vive en el título del propio recuadro,
                y el progreso medio de diez juegos distintos ya no se muestra:
                no responde a ninguna pregunta que se haga desde esta pantalla. */}
            <MetricStrip
              className="planner-summary"
              label="Resumen del plan"
              items={[
                {
                  id: "week",
                  label: "Plan semanal",
                  title: planSummaryDetail,
                  alert: metrics.weekOverloadMinutes > 0,
                  value: `${formatPlaytime(metrics.weekPlannedMinutes)} / ${formatPlaytime(
                    metrics.weekCapacityMinutes,
                  )}`,
                },
              ]}
            />
            {firstColumn ? (
              <PlannerAddGame column={firstColumn} plannedIds={plannedIds} label="Añadir juego" />
            ) : null}
          </>
        }
      />
      {message && (
        <p className="operation-message planner-message" role="status">
          {isSaving ? "Guardando plan…" : message}
          {lastMove && !isSaving && (
            <Button
              variant="outline"
              size="xs"
              onClick={undoLastMove}
              aria-label={`Deshacer el movimiento de ${lastMove.title}`}
            >
              <IconArrowBackUp /> Deshacer
            </Button>
          )}
        </p>
      )}
      <Tabs
        className="planner-workspace"
        value={mode}
        onValueChange={(value) => setMode(value as PlannerMode)}
      >
        <div className="planner-toolbar">
          <TabsList variant="line" aria-label="Vistas del planificador">
            <TabsTrigger value="kanban">Kanban</TabsTrigger>
            <TabsTrigger value="queue">Cola</TabsTrigger>
            <TabsTrigger value="week">Semana</TabsTrigger>
            <TabsTrigger value="month">Mes</TabsTrigger>
          </TabsList>
          {(mode === "week" || mode === "month") && (
            <div className="planner-toolbar__period">
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label={mode === "month" ? "Mes anterior" : "Semana anterior"}
                onClick={() => setAnchorIso((current) => shiftAnchor(current, mode, -1))}
              >
                <IconChevronLeft />
              </Button>
              <strong>{period.rangeLabel}</strong>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label={mode === "month" ? "Mes siguiente" : "Semana siguiente"}
                onClick={() => setAnchorIso((current) => shiftAnchor(current, mode, 1))}
              >
                <IconChevronRight />
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setAnchorIso(localIsoDate())}>
                Hoy
              </Button>
            </div>
          )}
          <div className="planner-toolbar__actions">
            {metrics.weekOverloadMinutes > 0 && (
              <Badge variant="destructive" className="planner-overload-badge">
                +{formatPlaytime(metrics.weekOverloadMinutes)} esta semana
              </Badge>
            )}
            <PlannerCapacityEditor settings={settings} onSave={saveCapacity} />
          </div>
        </div>
        <TabsContent value="kanban" className="planner-view-panel">
          {!columns.length ? (
            <EmptyState
              icon={IconLayoutKanban}
              title="El planificador aún no tiene columnas"
              description="Las columnas predeterminadas se crearán al inicializar la base local."
            />
          ) : (
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              accessibility={{
                announcements: plannerAnnouncements,
                screenReaderInstructions: plannerInstructions,
              }}
              onDragStart={onDragStart}
              onDragEnd={onDragEnd}
              onDragCancel={() => setActiveItem(undefined)}
            >
              <div className="kanban-board">
                {columns.map((column) => (
                  <PlannerLane
                    key={column.id}
                    column={column}
                    columns={columns}
                    plannedIds={plannedIds}
                    onEdit={saveItem}
                    busy={isSaving}
                    onMoveToColumn={(appId, columnId) => {
                      const destino = columns.find((candidate) => candidate.id === columnId);
                      mutation.mutate({
                        appId,
                        columnId,
                        position: destino ? destino.items.length : 0,
                      });
                    }}
                    onRemoveItem={(appId) => removeMutation.mutate(appId)}
                    onOpenDetail={setOpenGameId}
                    onChangeColor={(color) => columnMutation.mutate({ column, color })}
                    onChangeLimit={(wipLimit) => columnMutation.mutate({ column, wipLimit })}
                    {...(onOpenSettings
                      ? { onOpenSettings: () => onOpenSettings("organization") }
                      : {})}
                  />
                ))}
              </div>
              <DragOverlay>
                {activeItem ? <PlannerCard item={activeItem} overlay onEdit={saveItem} /> : null}
              </DragOverlay>
            </DndContext>
          )}
        </TabsContent>
        <TabsContent value="queue" className="planner-view-panel">
          <PlannerQueueView
            items={queue as PlannerViewItem[]}
            onMove={(appId, position) => queueMutation.mutate({ appId, position })}
            onEdit={saveItem}
            moving={queueMutation.isPending}
          />
        </TabsContent>
        <TabsContent value="week" className="planner-view-panel">
          <PlannerPeriodView period={period} label="Plan semanal" onEdit={saveItem} />
        </TabsContent>
        <TabsContent value="month" className="planner-view-panel">
          <PlannerPeriodView period={period} label="Plan mensual" onEdit={saveItem} />
        </TabsContent>
      </Tabs>
      {openGameId !== undefined && bootstrap ? (
        <Suspense fallback={null}>
          <GameDetailSheet
            appId={openGameId}
            open
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

function PlannerLane({
  column,
  columns,
  plannedIds,
  onEdit,
  busy,
  onMoveToColumn,
  onRemoveItem,
  onOpenDetail,
  onChangeColor,
  onChangeLimit,
  onOpenSettings,
}: {
  column: PlannerColumn;
  columns: readonly PlannerColumn[];
  plannedIds: Set<number>;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
  busy: boolean;
  onMoveToColumn: (appId: number, columnId: string) => void;
  onRemoveItem: (appId: number) => void;
  onOpenDetail: (appId: number) => void;
  onChangeColor: (color: string) => void;
  onChangeLimit: (limit: number | undefined) => void;
  onOpenSettings?: (() => void) | undefined;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: column.id });
  const overloaded = Boolean(column.wipLimit && column.items.length > column.wipLimit);
  return (
    <section className="planner-lane" data-over={isOver} ref={setNodeRef}>
      <PlannerLaneContextMenu
        column={column}
        busy={busy}
        onChangeColor={onChangeColor}
        onChangeLimit={onChangeLimit}
        {...(onOpenSettings ? { onOpenSettings } : {})}
      >
        <header>
          <span className="lane-color" style={{ backgroundColor: column.color }} />
          <h2 title={column.name}>{column.name}</h2>
          <data>{column.items.length}</data>
          {column.wipLimit && (
            <Badge variant={overloaded ? "destructive" : "outline"}>Límite {column.wipLimit}</Badge>
          )}
          <PlannerAddGame column={column} plannedIds={plannedIds} />
        </header>
      </PlannerLaneContextMenu>
      {overloaded && (
        <div className="lane-warning">
          <IconAlertTriangle size={14} /> Sobrecarga: reduce el trabajo activo.
        </div>
      )}
      <SortableContext
        items={column.items.map((item) => String(item.appId))}
        strategy={verticalListSortingStrategy}
      >
        <div className="planner-lane__items">
          {column.items.map((item) => (
            <SortablePlannerCard
              key={item.appId}
              item={item}
              onEdit={onEdit}
              columns={columns}
              currentColumnId={column.id}
              busy={busy}
              onMoveToColumn={(columnId) => onMoveToColumn(item.appId, columnId)}
              onRemove={() => onRemoveItem(item.appId)}
              onOpenDetail={onOpenDetail}
            />
          ))}
          {/* La zona de destino se muestra siempre: una columna con hueco
              tiene que anunciar que acepta juegos, no solo cuando está vacía. */}
          <div className="lane-empty" data-populated={column.items.length > 0}>
            <IconTargetArrow size={20} />
            <span>
              {column.items.length === 0
                ? "Añade o suelta aquí un juego"
                : "Suelta aquí para añadir a esta columna"}
            </span>
          </div>
        </div>
      </SortableContext>
    </section>
  );
}

function PlannerAddGame({
  column,
  plannedIds,
  label,
}: {
  column: PlannerColumn;
  plannedIds: Set<number>;
  /** Con etiqueta es la acción principal del encabezado; sin ella, el «+» de la columna. */
  label?: string;
}) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const games = useQuery({
    queryKey: ["planner-add", query],
    queryFn: () =>
      api.listGames({
        ...(query.trim() ? { query: query.trim() } : {}),
        sort: "alphabetical",
        limit: 60,
      }),
    enabled: open,
  });
  const add = useMutation({
    mutationFn: (appId: number) => api.movePlannerItem(appId, column.id, column.items.length),
    onSuccess: () => {
      setOpen(false);
      setQuery("");
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
  });
  const options = games.data?.items.filter((game) => !plannedIds.has(game.appId)) ?? [];
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        {label ? (
          // El nombre accesible contiene el texto visible y además dice a qué
          // columna cae el juego: la acción no puede ser un salto a ciegas.
          <Button aria-label={`${label} a ${column.name}`}>
            <IconPlus /> {label}
          </Button>
        ) : (
          <Button variant="ghost" size="icon-xs" aria-label={`Añadir juego a ${column.name}`}>
            <IconPlus />
          </Button>
        )}
      </PopoverTrigger>
      <PopoverContent align="end" className="planner-add-popover">
        <Command shouldFilter={false}>
          <CommandInput value={query} onValueChange={setQuery} placeholder="Buscar juego…" />
          <CommandList>
            <CommandEmpty>
              {games.isPending
                ? "Buscando…"
                : games.isError
                  ? `No se pudo cargar la biblioteca: ${getErrorMessage(games.error)}`
                  : "No hay juegos disponibles."}
            </CommandEmpty>
            <CommandGroup>
              {options.map((game) => (
                <CommandItem
                  key={game.appId}
                  value={String(game.appId)}
                  disabled={add.isPending}
                  onSelect={() => add.mutate(game.appId)}
                >
                  <Artwork
                    appId={game.appId}
                    src={game.iconUrl ?? game.coverUrl}
                    title={game.title}
                    kind="icon"
                  />
                  <span>{game.title}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
          {add.isError && (
            <p className="planner-add-error" role="alert">
              {getErrorMessage(add.error)}
            </p>
          )}
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function SortablePlannerCard({
  item,
  onEdit,
  columns,
  currentColumnId,
  busy,
  onMoveToColumn,
  onRemove,
  onOpenDetail,
}: {
  item: PlannerItem;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
  columns: readonly PlannerColumn[];
  currentColumnId: string;
  busy: boolean;
  onMoveToColumn: (columnId: string) => void;
  onRemove: () => void;
  onOpenDetail: (appId: number) => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: String(item.appId),
  });
  return (
    <PlannerCardContextMenu
      item={item}
      columns={columns}
      currentColumnId={currentColumnId}
      busy={busy}
      onMoveToColumn={onMoveToColumn}
      onRemove={onRemove}
      onOpenDetail={onOpenDetail}
      onSaveItem={onEdit}
    >
      {/* Sus capturas al detenerse. Se apaga durante el arrastre: un emergente
          siguiendo al puntero mientras se mueve una tarjeta estorba. */}
      <GamePreviewCard
        appId={item.appId}
        title={item.title}
        disabled={isDragging}
        fallback={
          <Artwork appId={item.appId} src={item.coverUrl} title={item.title} kind="cover" />
        }
        facts={[
          { label: "Progreso", value: `${item.progress} %` },
          ...(item.objective ? [{ label: "Objetivo", value: item.objective }] : []),
          ...(item.estimatedMinutes
            ? [{ label: "Estimado", value: formatPlaytime(item.estimatedMinutes) }]
            : []),
          ...(item.targetDate ? [{ label: "Fecha", value: formatDate(item.targetDate) }] : []),
        ]}
      >
        <div
          ref={setNodeRef}
          style={{ transform: CSS.Transform.toString(transform), transition }}
          data-dragging={isDragging}
          // El gesto de puntero vive en el contenedor: toda la tarjeta arrastra.
          onPointerDown={listeners?.onPointerDown as React.PointerEventHandler<HTMLDivElement>}
        >
          <PlannerCard item={item} dragProps={{ ...attributes, ...listeners }} onEdit={onEdit} />
        </div>
      </GamePreviewCard>
    </PlannerCardContextMenu>
  );
}

function PlannerCard({
  item,
  overlay,
  dragProps,
  onEdit,
}: {
  item: PlannerItem;
  overlay?: boolean;
  dragProps?: Record<string, unknown>;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
}) {
  return (
    <article className="planner-card" data-overlay={overlay}>
      <Artwork appId={item.appId} src={item.coverUrl} title={item.title} kind="cover" />
      <div className="planner-card__content">
        <div>
          <strong>{item.title}</strong>
          {/* Sin asa dibujada: la tarjeta entera arrastra con el puntero y este
              activador sólo existe para teclado y lectores de pantalla. */}
          <button
            type="button"
            className="planner-drag-activator"
            aria-label={`Mover ${item.title}`}
            {...dragProps}
          />
          {!overlay && <PlannerItemEditor item={item as PlannerViewItem} onSave={onEdit} />}
        </div>
        {item.objective && <p className="planner-card__objective">{item.objective}</p>}
        <ProgressMeter value={item.progress} label={`Progreso de ${item.title}`} />
        <div className="planner-card__meta">
          {item.estimatedMinutes && <span>{formatPlaytime(item.estimatedMinutes)}</span>}
          {item.targetDate && (
            <span>
              <IconCalendar size={12} /> {formatDate(item.targetDate)}
            </span>
          )}
        </div>
      </div>
    </article>
  );
}
