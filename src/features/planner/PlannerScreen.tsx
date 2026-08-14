import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
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
  IconCalendar,
  IconChevronLeft,
  IconChevronRight,
  IconGripVertical,
  IconLayoutKanban,
  IconPlus,
  IconRefresh,
  IconTargetArrow,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { EmptyState } from "@/components/common/EmptyState";
import { LoadingState } from "@/components/common/LoadingState";
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
import { Progress } from "@/components/ui/progress";
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
import {
  PlannerCapacityEditor,
  PlannerItemEditor,
  PlannerPeriodView,
  PlannerQueueView,
  type PlannerViewItem,
} from "./PlannerViews";
import { buildMonthSegments, buildPlannerMetrics, buildWeekSegments } from "./planner-periods";
import "./planner-advanced.css";

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
}: {
  bootstrap?: AppBootstrap;
  loading: boolean;
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
  const saveItem = async (input: SavePlannerItemInput) => itemMutation.mutateAsync(input);
  const saveCapacity = async (next: PlannerSettings) => {
    await capacityMutation.mutateAsync(next);
  };
  const isSaving =
    mutation.isPending ||
    queueMutation.isPending ||
    itemMutation.isPending ||
    capacityMutation.isPending;

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
    setColumns((current) =>
      current.map((column) => ({
        ...column,
        items:
          column.id === destination.id
            ? [
                ...column.items.filter((item) => item.appId !== appId).slice(0, position),
                movingItem,
                ...column.items.filter((item) => item.appId !== appId).slice(position),
              ].filter(Boolean)
            : column.items.filter((item) => item.appId !== appId),
      })),
    );
    mutation.mutate({ appId, columnId: destination.id, position });
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
      <header className="screen-heading">
        <div>
          <p className="eyebrow">PLAN DE JUEGO</p>
          <h1>Planificador</h1>
          <p>Convierte el backlog en una secuencia asumible y concreta.</p>
        </div>
        <div className="planner-summary">
          <div>
            <span>EN PLAN</span>
            <strong>{metrics.totalGames}</strong>
          </div>
          <div>
            <span>TIEMPO RESTANTE</span>
            <strong>{formatPlaytime(metrics.remainingMinutes)}</strong>
          </div>
          <div>
            <span>PROGRESO MEDIO</span>
            <strong>{metrics.averageProgress}%</strong>
          </div>
          <div data-overloaded={metrics.weekOverloadMinutes > 0}>
            <span>PLAN SEMANAL</span>
            <strong>
              {formatPlaytime(metrics.weekPlannedMinutes)} /{" "}
              {formatPlaytime(metrics.weekCapacityMinutes)}
            </strong>
          </div>
        </div>
      </header>
      {message && (
        <p className="operation-message planner-message" role="status">
          {isSaving ? "Guardando plan…" : message}
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
              onDragStart={onDragStart}
              onDragEnd={onDragEnd}
              onDragCancel={() => setActiveItem(undefined)}
            >
              <div className="kanban-board">
                {columns.map((column) => (
                  <PlannerLane
                    key={column.id}
                    column={column}
                    plannedIds={plannedIds}
                    onEdit={saveItem}
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
    </section>
  );
}

function PlannerLane({
  column,
  plannedIds,
  onEdit,
}: {
  column: PlannerColumn;
  plannedIds: Set<number>;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: column.id });
  const overloaded = Boolean(column.wipLimit && column.items.length > column.wipLimit);
  return (
    <section className="planner-lane" data-over={isOver} ref={setNodeRef}>
      <header>
        <span className="lane-color" style={{ backgroundColor: column.color }} />
        <h2>{column.name}</h2>
        <data>{column.items.length}</data>
        {column.wipLimit && (
          <Badge variant={overloaded ? "destructive" : "outline"}>Límite {column.wipLimit}</Badge>
        )}
        <PlannerAddGame column={column} plannedIds={plannedIds} />
      </header>
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
            <SortablePlannerCard key={item.appId} item={item} onEdit={onEdit} />
          ))}
          {column.items.length === 0 && (
            <div className="lane-empty">
              <IconTargetArrow size={20} />
              <span>Añade o suelta aquí un juego</span>
            </div>
          )}
        </div>
      </SortableContext>
    </section>
  );
}

function PlannerAddGame({
  column,
  plannedIds,
}: {
  column: PlannerColumn;
  plannedIds: Set<number>;
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
        <Button variant="ghost" size="icon-xs" aria-label={`Añadir juego a ${column.name}`}>
          <IconPlus />
        </Button>
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
}: {
  item: PlannerItem;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: String(item.appId),
  });
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      data-dragging={isDragging}
    >
      <PlannerCard item={item} dragProps={{ ...attributes, ...listeners }} onEdit={onEdit} />
    </div>
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
      <Artwork appId={item.appId} src={item.coverUrl} title={item.title} kind="icon" />
      <div className="planner-card__content">
        <div>
          <strong>{item.title}</strong>
          <button
            type="button"
            className="drag-handle"
            aria-label={`Mover ${item.title}`}
            {...dragProps}
          >
            <IconGripVertical size={16} />
          </button>
          {!overlay && <PlannerItemEditor item={item as PlannerViewItem} onSave={onEdit} />}
        </div>
        {item.objective && <p className="planner-card__objective">{item.objective}</p>}
        <Progress value={item.progress} aria-label={`Progreso ${item.progress}%`} />
        <div className="planner-card__meta">
          <span>{item.progress}%</span>
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
