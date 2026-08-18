import {
  type CollisionDetection,
  closestCenter,
  DndContext,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  pointerWithin,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import {
  IconArrowBackUp,
  IconBrandSteam,
  IconGripVertical,
  IconLoader2,
  IconRefresh,
  IconX,
} from "@tabler/icons-react";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
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
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FamilyCatalogBrowser } from "@/features/library/FamilyCatalogBrowser";
import { GameBrowser, type GameOrganizationHandlers } from "@/features/library/GameBrowser";
import { type LibraryScope, LibrarySidebar } from "@/features/library/LibrarySidebar";
import { type ExtraFilters, LibraryToolbar } from "@/features/library/LibraryToolbar";
import {
  draggedAppIds,
  parseCollectionOrderDragId,
  parseGameDragId,
  parseLibraryDropTarget,
  reorderCollectionIds,
} from "@/features/library/library-dnd";
import { activeLibraryFilterCount } from "@/features/library/library-filters";
import type { LibraryGrouping } from "@/features/library/library-grouping";
import { applySelectionGesture, type SelectionGesture } from "@/features/library/library-selection";
import {
  libraryScopeKey,
  readLibraryScroll,
  readLibrarySession,
  writeLibraryScroll,
  writeLibrarySession,
} from "@/features/library/library-session";
import {
  describeUndo,
  peekUndo,
  popUndo,
  pruneUndo,
  pushUndo,
  type UndoEntry,
} from "@/features/library/library-undo";
import {
  combinedPresentation,
  combineViews,
  type SavedLibraryView,
  type SavedViewQuery,
  toggleViewInStack,
} from "@/features/library/library-views";
import { SavedViewsBar } from "@/features/library/SavedViewsBar";
import {
  gameContextShortcuts,
  type LibraryContextSnapshot,
  onLibraryCommand,
  onLibraryContextRequest,
  publishLibraryContext,
  readLocalShortcuts,
  resolveShortcuts,
} from "@/features/shell/shortcuts";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  AppBootstrap,
  ArchiveScope,
  FamilyCatalogAvailability,
  FamilyCatalogSort,
  GameListRequest,
  GameSort,
  GameSummary,
  LibraryDropReceipt,
  LibraryDropTarget,
  LibraryView,
  UpdateGameInput,
} from "@/lib/types";
import emptyLibraryArtwork from "../../../assets/brand/vindexa-empty-library.png";
import "./library-dnd.css";

const GameDetailSheet = lazy(() =>
  import("@/features/library/GameDetailSheet").then((module) => ({
    default: module.GameDetailSheet,
  })),
);
const CollectionEditorDialog = lazy(() =>
  import("@/features/collections/CollectionEditorDialog").then((module) => ({
    default: module.CollectionEditorDialog,
  })),
);

interface Props {
  bootstrap?: AppBootstrap | undefined;
  loading: boolean;
  error: unknown;
  onRetry: () => void;
}

/**
 * Resolución del destino de un arrastre.
 *
 * Manda dónde está el puntero, no dónde queda el centro de lo que se arrastra:
 * una carátula mide más de trescientos píxeles de alto y con `closestCenter` el
 * juego caía en el destino de debajo del que la persona estaba señalando. Sólo
 * cuando no hay puntero —arrastre con teclado— se recurre al centro, que ahí sí
 * es la única referencia disponible.
 */
const dropCollision: CollisionDetection = (args) => {
  const bajoElPuntero = pointerWithin(args);
  return bajoElPuntero.length > 0 ? bajoElPuntero : closestCenter(args);
};

export function LibraryScreen({ bootstrap, loading, error, onRetry }: Props) {
  const queryClient = useQueryClient();
  const session = useMemo(readLibrarySession, []);
  const [scope, setScope] = useState<LibraryScope>(session.scope);
  const [query, setQuery] = useState(session.query);
  const debouncedQuery = useDebouncedValue(query);
  const [sort, setSort] = useState<GameSort>(
    session.sort === "manual" ? (bootstrap?.preferences.librarySort ?? session.sort) : session.sort,
  );
  const [randomSeed, setRandomSeed] = useState(session.randomSeed);
  const [view, setView] = useState<LibraryView>(session.view);
  const [grouping, setGrouping] = useState<LibraryGrouping>(session.grouping);
  const [familyAvailability, setFamilyAvailability] = useState<FamilyCatalogAvailability>(
    session.familyAvailability,
  );
  const [familySort, setFamilySort] = useState<FamilyCatalogSort>(session.familySort);
  const [filters, setFilters] = useState<ExtraFilters>(session.filters);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [detailId, setDetailId] = useState<number>();
  /** Cursor de teclado. `detailId` sigue siendo, solo, la ficha abierta. */
  const [focusedId, setFocusedId] = useState<number>();
  const [collectionEditorOpen, setCollectionEditorOpen] = useState(false);
  const [operationMessage, setOperationMessage] = useState<string>();
  const [undoReceipt, setUndoReceipt] = useState<LibraryDropReceipt>();
  /**
   * Pila unificada de deshacer. Convive con `undoReceipt` y `orderUndo`, que
   * son los que alimentan los botones puntuales de cada operación; la pila es
   * lo que consume el atajo, que no sabe qué tipo de cambio fue el último.
   */
  const [undoStack, setUndoStack] = useState<UndoEntry[]>([]);
  const [activeDrag, setActiveDrag] = useState<{
    appIds: number[];
    title: string;
    appId: number;
    coverUrl?: string | undefined;
  }>();
  const [activeCollectionDrag, setActiveCollectionDrag] = useState<{
    id: string;
    title: string;
  }>();
  const [collectionOrder, setCollectionOrder] = useState<string[]>([]);
  const [orderUndo, setOrderUndo] = useState<string[]>();
  const awaitingInitialPreferences = useRef(!bootstrap);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  useEffect(() => {
    writeLibrarySession({
      scope,
      query,
      sort,
      randomSeed,
      view,
      grouping,
      familyAvailability,
      familySort,
      filters,
    });
  }, [familyAvailability, familySort, filters, grouping, query, randomSeed, scope, sort, view]);
  useEffect(() => {
    if (bootstrap && awaitingInitialPreferences.current) {
      awaitingInitialPreferences.current = false;
      setSort(bootstrap.preferences.librarySort);
    }
  }, [bootstrap]);
  useEffect(() => {
    if (!bootstrap) return;
    setCollectionOrder(bootstrap.collections.map((collection) => collection.id));
  }, [bootstrap]);

  const sidebarBootstrap = useMemo(() => {
    if (!bootstrap || !collectionOrder.length) return bootstrap;
    const positions = new Map(collectionOrder.map((id, index) => [id, index]));
    return {
      ...bootstrap,
      collections: [...bootstrap.collections].sort(
        (left, right) =>
          (positions.get(left.id) ?? Number.MAX_SAFE_INTEGER) -
          (positions.get(right.id) ?? Number.MAX_SAFE_INTEGER),
      ),
    };
  }, [bootstrap, collectionOrder]);

  const filterOptionsQuery = useQuery({
    queryKey: ["library-filter-options"],
    queryFn: api.libraryFilterOptions,
    staleTime: 30_000,
  });

  /*
   * Archivar sólo significa algo si la vista por defecto esconde lo archivado.
   * El ámbito vive fuera de `filters` a propósito: no es un filtro más que se
   * combina con los otros, es la pregunta previa de qué biblioteca se está
   * mirando. Y no se persiste entre sesiones: volver a abrir Vindexa tiene que
   * devolverte a tu biblioteca, no al vertedero que decidiste esconder.
   */
  const [archiveScope, setArchiveScope] = useState<ArchiveScope>("active");
  const archivedCount = bootstrap?.stats.archivedGames ?? 0;

  const requestBase = useMemo<GameListRequest>(() => {
    const { statusId, collectionId, installed, ...advancedFilters } = filters;
    return {
      ...advancedFilters,
      ...(debouncedQuery.trim() ? { query: debouncedQuery.trim() } : {}),
      ...(statusId
        ? { statusId }
        : scope.kind === "status" && scope.id
          ? { statusId: scope.id }
          : {}),
      ...(collectionId
        ? { collectionId }
        : scope.kind === "collection" && scope.id
          ? { collectionId: scope.id }
          : {}),
      ...(scope.kind === "installed"
        ? { installed: true }
        : installed !== undefined
          ? { installed }
          : {}),
      ...(archiveScope === "active" ? {} : { archiveScope }),
      sort,
      ...(sort === "random" ? { sortSeed: randomSeed } : {}),
      limit: 240,
    };
  }, [archiveScope, debouncedQuery, filters, randomSeed, scope, sort]);
  const gamesQuery = useInfiniteQuery({
    queryKey: ["games", requestBase],
    queryFn: ({ pageParam }) => api.listGames({ ...requestBase, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (last) =>
      last.offset + last.items.length < last.total ? last.offset + last.items.length : undefined,
    enabled: scope.kind !== "family",
  });
  const familyQuery = useInfiniteQuery({
    queryKey: ["family-catalog", debouncedQuery, familyAvailability, familySort],
    queryFn: ({ pageParam }) =>
      api.listFamilyCatalog({
        ...(debouncedQuery.trim() ? { query: debouncedQuery.trim() } : {}),
        ...(familyAvailability === "all" ? {} : { availability: familyAvailability }),
        sort: familySort,
        limit: 240,
        offset: pageParam,
      }),
    initialPageParam: 0,
    getNextPageParam: (last) =>
      last.offset + last.items.length < last.total ? last.offset + last.items.length : undefined,
    enabled: scope.kind === "family",
  });
  const games = useMemo(
    () => gamesQuery.data?.pages.flatMap((page) => page.items) ?? [],
    [gamesQuery.data],
  );
  const metadataPriorityIds = useMemo(() => games.slice(-240).map((game) => game.appId), [games]);
  useEffect(() => {
    if (scope.kind === "family" || !metadataPriorityIds.length) return;
    void api
      .startMetadataEnrichment(metadataPriorityIds, true)
      .then((snapshot) => {
        queryClient.setQueryData(["metadata-enrichment"], snapshot);
      })
      .catch((cause) => {
        setOperationMessage(`No se pudo iniciar el índice de metadatos: ${getErrorMessage(cause)}`);
      });
  }, [metadataPriorityIds, queryClient, scope.kind]);
  const total = gamesQuery.data?.pages[0]?.total ?? 0;
  const familyGames = useMemo(
    () => familyQuery.data?.pages.flatMap((page) => page.items) ?? [],
    [familyQuery.data],
  );
  const familyTotal = familyQuery.data?.pages[0]?.total ?? 0;
  const visibleTotal = scope.kind === "family" ? familyTotal : total;
  const activeFilters =
    scope.kind === "family"
      ? Boolean(debouncedQuery || familyAvailability !== "all")
      : Boolean(debouncedQuery || activeLibraryFilterCount(filters) || scope.kind !== "all");

  useEffect(() => {
    const focusSearch = () => {
      document.querySelector<HTMLInputElement>(".search-field input")?.focus();
    };
    const closePanel = () => {
      if (detailId) setDetailId(undefined);
      else if (collectionEditorOpen) setCollectionEditorOpen(false);
      else if (selected.size) setSelected(new Set());
      else if (focusedId !== undefined) setFocusedId(undefined);
    };
    window.addEventListener("vindexa:focus-search", focusSearch);
    window.addEventListener("vindexa:close-panel", closePanel);
    return () => {
      window.removeEventListener("vindexa:focus-search", focusSearch);
      window.removeEventListener("vindexa:close-panel", closePanel);
    };
  }, [collectionEditorOpen, detailId, focusedId, selected.size]);

  const localImport = useMutation({
    mutationFn: api.importLocalSteam,
    onSuccess: (result) => {
      setOperationMessage(`${result.importedGames} juegos locales importados.`);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const steamSync = useMutation({
    mutationFn: api.syncSteamLibrary,
    onSuccess: (result) => {
      setOperationMessage(
        `${result.familyCatalogGamesDetected} juegos detectados en Steam Family; ${result.familyGamesImported} confirmados localmente.`,
      );
      void queryClient.invalidateQueries();
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const sortPreference = useMutation({
    mutationFn: api.savePreferences,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] }),
    onError: (cause) =>
      setOperationMessage(
        `La ordenación se aplicó, pero no se pudo guardar: ${getErrorMessage(cause)}`,
      ),
  });
  /**
   * Vistas guardadas. `activeViewIds` es una **pila**: varias vistas pueden
   * estar aplicadas a la vez y sus filtros se intersecan, en lugar de que la
   * última descarte a la anterior. El orden importa porque decide quién gana
   * los campos que no admiten dos valores.
   */
  /**
   * Juego cuya desinstalación espera confirmación. Teclado, paleta de comandos
   * y menú contextual pasan por aquí: la preferencia `confirmUninstall` debe
   * valer igual se pida desde donde se pida, y `Supr` es demasiado fácil de
   * pulsar sin querer sobre la ficha que tenga el foco.
   */
  const [uninstallTarget, setUninstallTarget] = useState<GameSummary | undefined>(undefined);
  const [activeViewIds, setActiveViewIds] = useState<string[]>([]);
  const [namingView, setNamingView] = useState(false);
  const savedViewsQuery = useQuery({
    queryKey: ["saved-views"],
    queryFn: api.listSavedViews,
    staleTime: 60_000,
  });
  const savedViews = useMemo(() => savedViewsQuery.data ?? [], [savedViewsQuery.data]);

  const currentViewQuery = useMemo<SavedViewQuery>(
    () => ({ search: query, sort, grouping, view, filters }),
    [query, sort, grouping, view, filters],
  );

  // Los conflictos se recalculan a partir de la pila, no se guardan: así nunca
  // quedan colgados de una vista que ya se retiró.
  const viewConflicts = useMemo(() => {
    const stack = activeViewIds
      .map((id) => savedViews.find((entry) => entry.id === id))
      .filter((entry): entry is SavedLibraryView => Boolean(entry));
    return combineViews(stack).conflicts;
  }, [activeViewIds, savedViews]);

  /**
   * El plan se lee aquí sólo para saber a qué columna va un juego añadido desde
   * la biblioteca; la pantalla del planificador sigue siendo su dueña.
   */
  const plannerColumnsQuery = useQuery({
    queryKey: ["planner-overview"],
    queryFn: api.getPlannerOverview,
    staleTime: 60_000,
  });
  const plannerColumns = useMemo(
    () => plannerColumnsQuery.data?.columns ?? [],
    [plannerColumnsQuery.data],
  );
  /*
   * Archivar en masa. Cuatrocientos juegos de paquete no se archivan de uno en
   * uno, así que la operación es una sola llamada con toda la selección: el
   * backend la valida entera antes de tocar nada y no deja lotes a medias.
   */
  const archiveSelection = useMutation({
    mutationFn: ({ appIds, archived }: { appIds: readonly number[]; archived: boolean }) =>
      archived ? api.archiveGames([...appIds]) : api.unarchiveGames([...appIds]),
    onSuccess: (report, { archived }) => {
      const verb = archived ? "archivado" : "devuelto a la biblioteca";
      const parts = [
        report.changed === 1 ? `1 juego ${verb}` : `${report.changed} juegos ${verb}s`,
      ];
      // Lo que no cambió se dice: anunciar un cambio que no ocurrió es mentir.
      if (report.unchanged > 0) {
        parts.push(
          report.unchanged === 1 ? "1 ya estaba así" : `${report.unchanged} ya estaban así`,
        );
      }
      // Archivar no saca nada del plan; si algo sigue ahí, se avisa en vez de
      // borrarlo en silencio.
      if (archived && report.inPlanner > 0) {
        parts.push(
          report.inPlanner === 1 ? "1 sigue en el plan" : `${report.inPlanner} siguen en el plan`,
        );
      }
      setOperationMessage(`${parts.join("; ")}.`);
      setSelected(new Set());
      void queryClient.invalidateQueries({ queryKey: ["games"] });
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
      void queryClient.invalidateQueries({ queryKey: ["library-filter-options"] });
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });

  const addToPlanner = useMutation({
    mutationFn: async ({
      appIds,
      columnId,
    }: {
      appIds: readonly number[];
      columnId?: string | undefined;
    }) => {
      const column = columnId ?? plannerColumns[0]?.id;
      if (!column) {
        throw new Error("El plan no tiene ninguna columna donde colocar el juego.");
      }
      // `move_planner_item` es también el alta: si el juego no estaba en el
      // plan, lo crea en la columna indicada.
      const start = plannerColumns.find((entry) => entry.id === column)?.items.length ?? 0;
      let added = 0;
      for (const [offset, appId] of appIds.entries()) {
        await api.movePlannerItem(appId, column, start + offset);
        added += 1;
      }
      return { added, column };
    },
    onSuccess: ({ added, column }) => {
      const name = plannerColumns.find((entry) => entry.id === column)?.name ?? "el plan";
      setOperationMessage(
        added === 1 ? `Añadido a ${name}.` : `${added} juegos añadidos a ${name}.`,
      );
      void queryClient.invalidateQueries({ queryKey: ["planner-overview"] });
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });

  const saveView = useMutation({
    mutationFn: api.saveSavedView,
    onSuccess: (saved) => {
      setOperationMessage(`Vista «${saved.name}» guardada.`);
      setActiveViewIds([saved.id]);
      void queryClient.invalidateQueries({ queryKey: ["saved-views"] });
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const deleteView = useMutation({
    mutationFn: api.deleteSavedView,
    onSuccess: (_data, viewId) => {
      setActiveViewIds((previous) => previous.filter((id) => id !== viewId));
      setOperationMessage("Vista eliminada.");
      void queryClient.invalidateQueries({ queryKey: ["saved-views"] });
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const markViewUsed = useMutation({
    mutationFn: api.markSavedViewUsed,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["saved-views"] }),
    // Que falle el contador de uso no debe estorbar: la vista ya se aplicó.
    onError: () => undefined,
  });

  /**
   * Aplica la pila resultante a la pantalla. Se llama con la lista ya calculada
   * para que alternar y restaurar compartan exactamente el mismo camino.
   */
  const applyViewStack = useCallback(
    (ids: string[]) => {
      setActiveViewIds(ids);
      const stack = ids
        .map((id) => savedViews.find((entry) => entry.id === id))
        .filter((entry): entry is SavedLibraryView => Boolean(entry));
      const combined = combineViews(stack);
      const presentation = combinedPresentation(stack);
      setFilters(combined.filters);
      setQuery(combined.search);
      if (presentation.sort) setSort(presentation.sort);
      if (presentation.grouping) setGrouping(presentation.grouping);
      if (presentation.view) setView(presentation.view);
    },
    [savedViews],
  );

  const toggleSavedView = useCallback(
    (target: SavedLibraryView) => {
      const next = toggleViewInStack(activeViewIds, target.id);
      applyViewStack(next);
      if (next.includes(target.id)) markViewUsed.mutate(target.id);
    },
    [activeViewIds, applyViewStack, markViewUsed],
  );

  const changeSort = (nextSort: GameSort) => {
    setSort(nextSort);
    if (nextSort === "random") {
      setRandomSeed(crypto.getRandomValues(new Uint32Array(1))[0] ?? 0);
    }
    if (bootstrap) {
      sortPreference.mutate({ ...bootstrap.preferences, librarySort: nextSort });
    }
  };
  /**
   * Operaciones del menú contextual. Se resuelven aquí porque necesitan el
   * bootstrap completo (estados y colecciones) y porque el mensaje de resultado
   * y la invalidación de consultas ya viven en esta pantalla.
   */
  const organizeGame = useMutation({
    mutationFn: ({ input }: { input: UpdateGameInput; message: string }) => api.updateGame(input),
    onSuccess: (_data, variables) => {
      setOperationMessage(variables.message);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const organizeCollections = useMutation({
    mutationFn: ({
      appId,
      collectionIds,
    }: {
      appId: number;
      collectionIds: string[];
      message: string;
    }) => api.setGameCollections(appId, collectionIds),
    onSuccess: (_data, variables) => {
      setOperationMessage(variables.message);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const collectionIdsByApp = useMemo(() => {
    const index = new Map<number, readonly string[]>();
    for (const game of games) index.set(game.appId, game.collectionIds ?? []);
    return index;
  }, [games]);
  /**
   * Contexto que la paleta de comandos y los atajos necesitan para operar sobre
   * el juego enfocado. Se publica al cambiar y se vuelve a publicar cuando el
   * shell lo pide, para que abrir la paleta nunca encuentre datos rancios.
   */
  const librarySnapshot = useMemo<LibraryContextSnapshot>(
    () => ({
      games,
      focusedAppId: focusedId,
      selectedAppIds: [...selected],
      statuses: bootstrap?.statuses ?? [],
      collections: bootstrap?.collections ?? [],
      collectionIdsByApp,
      plannerColumns: plannerColumns.map((column) => ({ id: column.id, name: column.name })),
      view,
      scopeLabel: scope.label,
    }),
    [bootstrap, collectionIdsByApp, focusedId, games, plannerColumns, scope.label, selected, view],
  );
  useEffect(() => {
    publishLibraryContext(librarySnapshot);
    return onLibraryContextRequest(() => publishLibraryContext(librarySnapshot));
  }, [librarySnapshot]);

  /** `updateGame` reemplaza el registro personal completo: cualquier cambio
   *  puntual debe partir del estado vigente del juego para no borrar el resto. */
  const personalUpdate = useCallback(
    (game: GameSummary, patch: Partial<UpdateGameInput>): UpdateGameInput => ({
      appId: game.appId,
      statusId: game.statusId,
      progress: game.progress,
      priority: game.priority,
      pinned: game.pinned,
      tracking: game.tracking,
      rating: game.rating,
      estimatedMinutes: game.estimatedMinutes,
      targetDate: game.targetDate,
      nextAction: game.nextAction,
      checkpoint: game.checkpoint,
      notes: game.notes,
      ...patch,
    }),
    [],
  );
  /**
   * Congela el estado personal vigente antes de tocarlo. `personalUpdate` sin
   * parche es exactamente lo que hay ahora, así que aplicarlo de vuelta deshace
   * el cambio sin depender de qué campo se modificó.
   */
  const recordGameEdit = useCallback(
    (game: GameSummary, label: string) => {
      const previous = personalUpdate(game, {});
      setUndoStack((stack) => pushUndo(stack, { kind: "gameEdit", label, previous }));
    },
    [personalUpdate],
  );
  const organization: GameOrganizationHandlers = useMemo(
    () => ({
      statuses: bootstrap?.statuses,
      collections: bootstrap?.collections,
      collectionIdsByApp,
      onChangeStatus: (game, statusId) => {
        const status = bootstrap?.statuses.find((candidate) => candidate.id === statusId);
        recordGameEdit(game, `${game.title} pasó a «${status?.name ?? statusId}»`);
        organizeGame.mutate({
          input: personalUpdate(game, { statusId }),
          message: `${game.title} pasó a «${status?.name ?? statusId}».`,
        });
      },
      onChangePriority: (game, priority) => {
        recordGameEdit(game, `prioridad de ${game.title}`);
        return organizeGame.mutate({
          input: personalUpdate(game, { priority }),
          message: `Prioridad de ${game.title} fijada en ${priority}.`,
        });
      },
      onTogglePinned: (game, pinned) => {
        recordGameEdit(game, `${game.title} ${pinned ? "fijado" : "desfijado"}`);
        return organizeGame.mutate({
          input: personalUpdate(game, { pinned }),
          message: pinned
            ? `${game.title} fijado en la biblioteca.`
            : `${game.title} ya no está fijado.`,
        });
      },
      onToggleTracking: (game, tracking) => {
        recordGameEdit(game, `seguimiento de ${game.title}`);
        return organizeGame.mutate({
          input: personalUpdate(game, { tracking }),
          message: tracking
            ? `${game.title} añadido a seguimiento.`
            : `${game.title} salió de seguimiento.`,
        });
      },
      onToggleCollection: (game, collectionId, member) => {
        const current = collectionIdsByApp.get(game.appId) ?? [];
        const next = member
          ? Array.from(new Set([...current, collectionId]))
          : current.filter((id) => id !== collectionId);
        const collection = bootstrap?.collections.find(
          (candidate) => candidate.id === collectionId,
        );
        organizeCollections.mutate({
          appId: game.appId,
          collectionIds: next,
          message: member
            ? `${game.title} añadido a «${collection?.name ?? "la colección"}».`
            : `${game.title} retirado de «${collection?.name ?? "la colección"}».`,
        });
      },
    }),
    [
      bootstrap,
      collectionIdsByApp,
      organizeCollections,
      organizeGame,
      personalUpdate,
      recordGameEdit,
    ],
  );

  /**
   * Ejecuta las órdenes que emiten los atajos y la paleta de comandos. Es el
   * único punto que traduce una intención de teclado en una operación real, con
   * el mismo feedback que las acciones de ratón.
   */
  const undoLastRef = useRef<() => void>(() => undefined);
  const runGameAction = useCallback(
    (game: GameSummary, pending: string, done: string, operation: () => Promise<unknown>) => {
      setOperationMessage(pending);
      void operation()
        .then(() => setOperationMessage(done))
        .catch((cause) => setOperationMessage(`${game.title}: ${getErrorMessage(cause)}`));
    },
    [],
  );
  useEffect(() => {
    return onLibraryCommand((command) => {
      if (command.kind === "setView") return setView(command.view);
      if (command.kind === "selectAll") {
        setSelected(new Set(games.map((candidate) => candidate.appId)));
        return;
      }
      // El movimiento del cursor lo resuelve el navegador, que conoce columnas.
      if (command.kind === "moveFocus") return;
      if (command.kind === "addToPlanner") {
        addToPlanner.mutate({ appIds: command.appIds, columnId: command.columnId });
        return;
      }
      if (command.kind === "undo") {
        // Se llama por referencia: `undoLast` depende de mutaciones declaradas
        // más abajo, y adelantarlas sólo por el orden léxico enredaría el resto.
        undoLastRef.current();
        return;
      }
      // La ficha sólo necesita el AppID: un resultado que la paleta encontró en
      // el catálogo completo no está en la página cargada y aun así debe abrirse.
      if (command.kind === "openDetail") {
        setDetailId(command.appId);
        return;
      }
      const game = games.find((candidate) => candidate.appId === command.appId);
      if (!game) return;
      switch (command.kind) {
        case "focus":
          setFocusedId(game.appId);
          return;
        case "primary":
          if (game.installed) {
            runGameAction(
              game,
              `Abriendo ${game.title}…`,
              `Steam recibió la solicitud para iniciar ${game.title}.`,
              () => api.launchGame(game.appId),
            );
          } else {
            setDetailId(game.appId);
          }
          return;
        case "play":
          runGameAction(
            game,
            `Abriendo ${game.title}…`,
            `Steam recibió la solicitud para iniciar ${game.title}.`,
            () => api.launchGame(game.appId),
          );
          return;
        case "install":
          runGameAction(
            game,
            `Preparando la instalación de ${game.title}…`,
            `Steam recibió la solicitud para instalar ${game.title}.`,
            () => api.installGame(game.appId),
          );
          return;
        case "uninstall":
          if (bootstrap?.preferences.confirmUninstall ?? true) {
            setUninstallTarget(game);
            return;
          }
          runGameAction(
            game,
            `Solicitando la desinstalación de ${game.title}…`,
            `Steam recibió la solicitud para desinstalar ${game.title}.`,
            () => api.uninstallGame(game.appId),
          );
          return;
        case "openStore":
          runGameAction(
            game,
            "Abriendo la tienda protegida…",
            `La tienda oficial de ${game.title} se abrió en una sesión privada.`,
            () => api.openStore(game.appId),
          );
          return;
        case "reveal":
          runGameAction(
            game,
            "Abriendo la carpeta de instalación…",
            `Se abrió la instalación de ${game.title}.`,
            () => api.revealInstallation(game.appId),
          );
          return;
        case "setStatus":
          organization.onChangeStatus?.(game, command.statusId);
          return;
        case "cycleStatus": {
          const list = bootstrap?.statuses ?? [];
          if (!list.length) return;
          const index = list.findIndex((status) => status.id === game.statusId);
          const next = list[(index + command.direction + list.length) % list.length];
          if (next) organization.onChangeStatus?.(game, next.id);
          return;
        }
        case "setPriority":
          organization.onChangePriority?.(game, command.priority);
          return;
        case "shiftPriority":
          organization.onChangePriority?.(
            game,
            Math.min(5, Math.max(0, game.priority + command.direction)),
          );
          return;
        case "togglePinned":
          organization.onTogglePinned?.(game, !game.pinned);
          return;
        case "toggleTracking":
          organization.onToggleTracking?.(game, !game.tracking);
          return;
        case "toggleCollection": {
          const member = !(collectionIdsByApp.get(game.appId) ?? []).includes(command.collectionId);
          organization.onToggleCollection?.(game, command.collectionId, member);
          return;
        }
        case "copyTitle":
          navigator.clipboard?.writeText(game.title).catch(() => undefined);
          setOperationMessage(`Título de ${game.title} copiado.`);
          return;
        case "copyAppId":
          navigator.clipboard?.writeText(String(game.appId)).catch(() => undefined);
          setOperationMessage(`AppID ${game.appId} copiado.`);
          return;
        default:
          return;
      }
    });
  }, [addToPlanner.mutate, bootstrap, collectionIdsByApp, games, organization, runGameAction]);
  const gameShortcuts = useMemo(
    () =>
      gameContextShortcuts(
        resolveShortcuts(bootstrap?.preferences.shortcuts, readLocalShortcuts()),
      ),
    [bootstrap?.preferences.shortcuts],
  );

  const libraryDrop = useMutation({
    mutationFn: ({
      appIds,
      target,
      label,
    }: {
      appIds: number[];
      target: LibraryDropTarget;
      label: string;
    }) => api.applyLibraryDrop({ appIds, target }).then((result) => ({ result, label })),
    onSuccess: ({ result, label }) => {
      setOperationMessage(
        `${result.moved} juego${result.moved === 1 ? "" : "s"} movido${result.moved === 1 ? "" : "s"} a ${label}.`,
      );
      setUndoReceipt(result.receipt);
      setUndoStack((stack) =>
        pushUndo(stack, {
          kind: "drop",
          label: `${result.moved} juego${result.moved === 1 ? "" : "s"} a ${label}`,
          receipt: result.receipt,
        }),
      );
      setOrderUndo(undefined);
      setSelected(new Set());
      void queryClient.invalidateQueries();
    },
    onError: (cause) => setOperationMessage(getErrorMessage(cause)),
  });
  const undoDrop = useMutation({
    mutationFn: (receipt: LibraryDropReceipt) => api.undoLibraryDrop(receipt),
    onSuccess: (restored) => {
      setOperationMessage(
        `Se restauró la organización anterior de ${restored} juego${restored === 1 ? "" : "s"}.`,
      );
      setUndoReceipt(undefined);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => {
      setOperationMessage(`No se pudo deshacer: ${getErrorMessage(cause)}`);
      setUndoReceipt(undefined);
    },
  });
  const reorderCollections = useMutation({
    mutationFn: ({ next }: { previous: string[]; next: string[] }) => api.reorderCollections(next),
    onSuccess: () => {
      setOperationMessage("Orden de colecciones guardado.");
      void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    },
    onError: (cause, variables) => {
      setCollectionOrder(variables.previous);
      setOperationMessage(`No se pudo reordenar: ${getErrorMessage(cause)}`);
    },
  });
  /**
   * Deshace el último cambio de la pila, sea del tipo que sea. Cada rama delega
   * en la mutación que ya sabe revertir ese cambio, así que no hay una segunda
   * ruta de escritura que pueda divergir de la primera.
   */
  /**
   * Las ediciones de juegos que ya no están en la página cargada no se pueden
   * deshacer con sentido: se retiran para que la acción no exista en lugar de
   * existir y fallar.
   */
  useEffect(() => {
    const conocidos = new Set(games.map((game) => game.appId));
    setUndoStack((stack) => {
      const podada = pruneUndo(stack, conocidos);
      return podada.length === stack.length ? stack : podada;
    });
  }, [games]);
  const undoLabel = describeUndo(peekUndo(undoStack));

  const undoLast = useCallback(() => {
    const { entry, rest } = popUndo(undoStack);
    if (!entry) {
      setOperationMessage("No hay nada que deshacer.");
      return;
    }
    setUndoStack(rest);
    switch (entry.kind) {
      case "drop":
        setUndoReceipt(undefined);
        undoDrop.mutate(entry.receipt);
        return;
      case "collectionOrder": {
        const current = collectionOrder.length
          ? collectionOrder
          : (bootstrap?.collections.map((collection) => collection.id) ?? []);
        setCollectionOrder(entry.previous);
        setOrderUndo(undefined);
        reorderCollections.mutate({ previous: current, next: entry.previous });
        return;
      }
      case "gameEdit":
        organizeGame.mutate({
          input: entry.previous,
          message: `Se deshizo: ${entry.label}`,
        });
        return;
    }
  }, [bootstrap, collectionOrder, organizeGame, reorderCollections, undoDrop, undoStack]);

  useEffect(() => {
    undoLastRef.current = undoLast;
  }, [undoLast]);

  const undoCollectionOrder = () => {
    if (!orderUndo) return;
    const previous = collectionOrder.length
      ? collectionOrder
      : (bootstrap?.collections.map((collection) => collection.id) ?? []);
    setCollectionOrder(orderUndo);
    setOrderUndo(undefined);
    reorderCollections.mutate({ previous, next: orderUndo });
  };
  const moveSelection = (target: LibraryDropTarget, label: string) => {
    const appIds = Array.from(selected);
    if (!appIds.length) return;
    libraryDrop.mutate({ appIds, target, label });
  };
  const onDragStart = (event: DragStartEvent) => {
    const collectionId = parseCollectionOrderDragId(event.active.id);
    if (collectionId) {
      setActiveCollectionDrag({
        id: collectionId,
        title: String(event.active.data.current?.title ?? "Colección"),
      });
      setActiveDrag(undefined);
      return;
    }
    const appId = parseGameDragId(event.active.id);
    if (!appId) return;
    const appIds = draggedAppIds(appId, selected);
    const title = String(event.active.data.current?.title ?? "Juego");
    const rawCover = event.active.data.current?.coverUrl;
    if (!selected.has(appId)) setSelected(new Set([appId]));
    setActiveDrag({
      appIds,
      title,
      appId,
      coverUrl: typeof rawCover === "string" ? rawCover : undefined,
    });
  };
  const onDragEnd = (event: DragEndEvent) => {
    if (activeCollectionDrag) {
      const dragged = activeCollectionDrag;
      setActiveCollectionDrag(undefined);
      const rawOverId = event.over ? String(event.over.id) : "";
      const overId =
        parseCollectionOrderDragId(rawOverId) ??
        (rawOverId.startsWith("collection:") ? rawOverId.slice("collection:".length) : undefined);
      if (!overId) {
        setOperationMessage("Reordenación de colecciones cancelada.");
        return;
      }
      const previous = collectionOrder.length
        ? collectionOrder
        : (bootstrap?.collections.map((collection) => collection.id) ?? []);
      const next = reorderCollectionIds(previous, dragged.id, overId);
      if (next === previous) return;
      setCollectionOrder(next);
      reorderCollections.mutate(
        { previous, next },
        {
          onSuccess: () => {
            setOrderUndo(previous);
            setUndoStack((stack) =>
              pushUndo(stack, {
                kind: "collectionOrder",
                label: "reordenar colecciones",
                previous,
              }),
            );
          },
        },
      );
      return;
    }
    const current = activeDrag;
    setActiveDrag(undefined);
    if (!current || !event.over) {
      setOperationMessage("Movimiento cancelado. Suelta sobre un estado o una colección manual.");
      return;
    }
    const destination = parseLibraryDropTarget(
      event.over.id,
      bootstrap?.statuses ?? [],
      bootstrap?.collections ?? [],
    );
    if (!destination) {
      setOperationMessage(
        "Destino no permitido. Las colecciones inteligentes se actualizan mediante sus reglas.",
      );
      return;
    }
    if (
      destination.target.kind === "manual" ||
      (destination.target.kind === "collection" && destination.target.beforeAppId)
    ) {
      setSort("manual");
    }
    libraryDrop.mutate({ appIds: current.appIds, ...destination });
  };
  const changeScope = (nextScope: LibraryScope) => {
    setSelected(new Set());
    setScope(nextScope);
    const collection = bootstrap?.collections.find(
      (candidate) => candidate.id === nextScope.id && candidate.kind === "manual",
    );
    if (nextScope.kind === "collection" && collection) setSort("manual");
  };
  const describeDropTarget = (value: string | number | undefined) => {
    if (value === undefined) return "Sin destino.";
    const collectionOrderId = parseCollectionOrderDragId(value);
    if (collectionOrderId) {
      const collection = bootstrap?.collections.find((item) => item.id === collectionOrderId);
      return collection ? `Posición de ${collection.name}.` : "Posición de colección.";
    }
    const destination = parseLibraryDropTarget(
      value,
      bootstrap?.statuses ?? [],
      bootstrap?.collections ?? [],
    );
    if (destination) return `Destino ${destination.label}.`;
    const raw = String(value);
    if (raw.startsWith("collection:")) {
      const collection = bootstrap?.collections.find(
        (item) => item.id === raw.slice("collection:".length),
      );
      if (collection?.kind === "smart") {
        return `Colección inteligente ${collection.name}; destino no permitido.`;
      }
    }
    return "Destino no permitido.";
  };
  /**
   * Ancla de la selección por rango: el último juego tocado sin Mayús. La
   * lógica vive en `library-selection.ts` para poder probarse sin montar la
   * pantalla entera.
   */
  const selectionAnchor = useRef<number | undefined>(undefined);
  const selectGame = (game: GameSummary, gesture: SelectionGesture) => {
    setSelected((current) => {
      const next = applySelectionGesture(
        games,
        { selected: current, anchor: selectionAnchor.current },
        game.appId,
        gesture,
      );
      selectionAnchor.current = next.anchor;
      return new Set(next.selected);
    });
    setFocusedId(game.appId);
  };

  if (loading && !bootstrap)
    return (
      <div className="library-layout">
        <LibrarySidebar
          bootstrap={sidebarBootstrap}
          scope={scope}
          onScopeChange={changeScope}
          onCreateCollection={() => setCollectionEditorOpen(true)}
        />
        <section className="library-main">
          <LibraryToolbar
            mode={scope.kind === "family" ? "family" : "library"}
            title={scope.label}
            query={query}
            onQueryChange={setQuery}
            sort={sort}
            onSortChange={changeSort}
            view={view}
            onViewChange={setView}
            grouping={grouping}
            onGroupingChange={setGrouping}
            filters={filters}
            onFiltersChange={setFilters}
            statuses={[]}
            collections={[]}
            filterOptions={filterOptionsQuery.data}
          />
          <LibrarySkeleton />
        </section>
      </div>
    );
  if (error && !bootstrap)
    return (
      <div className="library-layout">
        <LibrarySidebar
          bootstrap={sidebarBootstrap}
          scope={scope}
          onScopeChange={changeScope}
          onCreateCollection={() => setCollectionEditorOpen(true)}
        />
        <section className="library-main">
          <div className="screen-error">
            <IconRefresh />
            <h1>No se pudo abrir la biblioteca</h1>
            <p>{getErrorMessage(error)}</p>
            <Button onClick={onRetry}>Reintentar</Button>
          </div>
        </section>
      </div>
    );

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={dropCollision}
      accessibility={{
        screenReaderInstructions: {
          draggable:
            "Pulsa espacio para recoger el juego, usa las flechas para elegir un estado o colección manual y vuelve a pulsar espacio para soltar.",
        },
        announcements: {
          onDragStart: ({ active }) =>
            parseCollectionOrderDragId(active.id)
              ? `Has recogido la colección ${String(active.data.current?.title ?? "seleccionada")}.`
              : `Has recogido ${String(active.data.current?.title ?? "el juego")}.`,
          onDragOver: ({ over }) => describeDropTarget(over?.id),
          onDragEnd: ({ over }) =>
            over ? "Juego soltado. Aplicando movimiento." : "Movimiento cancelado.",
          onDragCancel: () => "Movimiento cancelado.",
        },
      }}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragCancel={() => {
        setActiveDrag(undefined);
        setActiveCollectionDrag(undefined);
      }}
    >
      <div className="library-layout">
        <LibrarySidebar
          bootstrap={sidebarBootstrap}
          scope={scope}
          onScopeChange={changeScope}
          onCreateCollection={() => setCollectionEditorOpen(true)}
          draggingGames={Boolean(activeDrag)}
          collectionReorderEnabled
        />
        <section className="library-main">
          <LibraryToolbar
            mode={scope.kind === "family" ? "family" : "library"}
            title={scope.label}
            total={visibleTotal}
            query={query}
            onQueryChange={setQuery}
            sort={sort}
            onSortChange={changeSort}
            view={view}
            onViewChange={setView}
            grouping={grouping}
            onGroupingChange={setGrouping}
            filters={filters}
            onFiltersChange={setFilters}
            statuses={bootstrap?.statuses ?? []}
            collections={bootstrap?.collections ?? []}
            filterOptions={filterOptionsQuery.data}
            onSaveView={scope.kind === "family" ? undefined : () => setNamingView(true)}
            saveViewLabel={
              activeViewIds.length > 1 ? "Guardar la combinación" : "Guardar esta vista"
            }
          />
          {scope.kind !== "family" && (
            <SavedViewsBar
              creating={namingView}
              onCreatingChange={setNamingView}
              views={savedViews}
              activeIds={activeViewIds}
              onToggle={toggleSavedView}
              onSave={(name) => saveView.mutate({ name, query: currentViewQuery, pinned: false })}
              onUpdate={(target) =>
                saveView.mutate({
                  id: target.id,
                  name: target.name,
                  description: target.description,
                  icon: target.icon,
                  accent: target.accent,
                  query: currentViewQuery,
                  pinned: target.pinned,
                })
              }
              onDelete={(target) => deleteView.mutate(target.id)}
              onTogglePinned={(target) =>
                saveView.mutate({
                  id: target.id,
                  name: target.name,
                  description: target.description,
                  icon: target.icon,
                  accent: target.accent,
                  query: target.query,
                  pinned: !target.pinned,
                })
              }
              currentQuery={currentViewQuery}
              conflicts={viewConflicts}
              statuses={bootstrap?.statuses ?? []}
              collections={bootstrap?.collections ?? []}
            />
          )}
          {/*
            El archivado no puede ser un agujero negro: mientras haya algo
            archivado, la biblioteca lo dice y ofrece verlo. Sin archivados la
            franja no existe, para no cobrar espacio por una función que esta
            biblioteca todavía no usa.
          */}
          {scope.kind !== "family" && (archivedCount > 0 || archiveScope !== "active") && (
            <fieldset className="library-archive-bar">
              <legend className="library-archive-bar__count">
                {archivedCount === 1 ? "1 juego archivado" : `${archivedCount} juegos archivados`}
              </legend>
              {(
                [
                  ["active", "Ocultarlos"],
                  ["archived", "Ver solo archivados"],
                  ["all", "Ver todo"],
                ] as const
              ).map(([value, label]) => (
                <Button
                  key={value}
                  size="xs"
                  variant={archiveScope === value ? "secondary" : "ghost"}
                  aria-pressed={archiveScope === value}
                  onClick={() => {
                    setArchiveScope(value);
                    setSelected(new Set());
                  }}
                >
                  {label}
                </Button>
              ))}
            </fieldset>
          )}
          {(scope.kind === "family" ? familyQuery.isPending : gamesQuery.isPending) ? (
            <LibrarySkeleton />
          ) : (scope.kind === "family" ? familyQuery.isError : gamesQuery.isError) ? (
            <div className="screen-error">
              <IconRefresh />
              <h2>No se pudieron cargar los juegos</h2>
              <p>
                {getErrorMessage(scope.kind === "family" ? familyQuery.error : gamesQuery.error)}
              </p>
              <Button
                onClick={() =>
                  scope.kind === "family" ? familyQuery.refetch() : gamesQuery.refetch()
                }
              >
                Reintentar
              </Button>
            </div>
          ) : visibleTotal === 0 ? (
            <div className="library-empty">
              <div className="library-empty__visual">
                <img src={emptyLibraryArtwork} alt="" />
              </div>
              <div>
                <p className="eyebrow">
                  {scope.kind === "family" ? "CATÁLOGO COMPARTIDO" : "TU ÍNDICE PERSONAL"}
                </p>
                <h1>
                  {activeFilters
                    ? "Ningún juego coincide"
                    : scope.kind === "family"
                      ? "Sin catálogo de Steam Family"
                      : "Construye tu biblioteca real"}
                </h1>
                <p>
                  {activeFilters
                    ? "Prueba a retirar algún filtro o cambia el término de búsqueda."
                    : scope.kind === "family"
                      ? "Sincroniza Steam manualmente. Vindexa mostrará los títulos visibles del grupo sin afirmar que todos sean jugables."
                      : "Importa los manifiestos instalados en este equipo. Después podrás completar tiempo jugado, logros y portadas vinculando Steam."}
                </p>
              </div>
              {activeFilters ? (
                <Button
                  onClick={() => {
                    setQuery("");
                    if (scope.kind === "family") {
                      setFamilyAvailability("all");
                    } else {
                      setFilters({});
                      setScope({ kind: "all", label: "Todos los juegos" });
                    }
                  }}
                >
                  <IconX /> Limpiar búsqueda y filtros
                </Button>
              ) : scope.kind === "family" ? (
                <Button onClick={() => steamSync.mutate()} disabled={steamSync.isPending}>
                  {steamSync.isPending ? (
                    <IconLoader2 className="is-spinning" />
                  ) : (
                    <IconBrandSteam />
                  )}{" "}
                  Sincronizar Steam Family
                </Button>
              ) : (
                <Button onClick={() => localImport.mutate()} disabled={localImport.isPending}>
                  {localImport.isPending ? (
                    <IconLoader2 className="is-spinning" />
                  ) : (
                    <IconBrandSteam />
                  )}{" "}
                  Importar Steam local
                </Button>
              )}
              {operationMessage && (
                <p className="operation-message" role="status">
                  {operationMessage}
                </p>
              )}
            </div>
          ) : scope.kind === "family" ? (
            <FamilyCatalogBrowser
              games={familyGames}
              total={familyTotal}
              view={view}
              availability={familyAvailability}
              sort={familySort}
              queryKey={debouncedQuery.trim()}
              hasMore={Boolean(familyQuery.hasNextPage)}
              loadingMore={familyQuery.isFetchingNextPage}
              initialScrollOffset={readLibraryScroll(scope)}
              onScrollOffsetChange={(offset) => writeLibraryScroll(scope, offset)}
              onAvailabilityChange={(availability) => {
                writeLibraryScroll(scope, 0);
                setFamilyAvailability(availability);
              }}
              onSortChange={(nextSort) => {
                writeLibraryScroll(scope, 0);
                setFamilySort(nextSort);
              }}
              onViewChange={setView}
              onLoadMore={() => familyQuery.fetchNextPage()}
              onOpenConfirmed={setDetailId}
            />
          ) : (
            <GameBrowser
              key={`${libraryScopeKey(scope)}:${view}`}
              games={games}
              total={total}
              view={view}
              selected={selected}
              focusedGameId={focusedId ?? detailId}
              shortcuts={gameShortcuts}
              onMoveFocus={(appId, extend) => {
                const game = games.find((candidate) => candidate.appId === appId);
                if (!game) return;
                selectGame(game, extend ? "range" : "replace");
              }}
              hasMore={Boolean(gamesQuery.hasNextPage)}
              loadingMore={gamesQuery.isFetchingNextPage}
              initialScrollOffset={readLibraryScroll(scope)}
              onScrollOffsetChange={(offset) => writeLibraryScroll(scope, offset)}
              onLoadMore={() => gamesQuery.fetchNextPage()}
              onSelect={selectGame}
              onOpen={(game) => setDetailId(game.appId)}
              manualPositioning={sort === "manual" && scope.kind === "all"}
              organization={organization}
              grouping={grouping}
              positionCollectionId={
                sort === "manual" &&
                scope.kind === "collection" &&
                bootstrap?.collections.some(
                  (collection) => collection.id === scope.id && collection.kind === "manual",
                )
                  ? scope.id
                  : undefined
              }
            />
          )}
          {scope.kind !== "family" && selected.size > 0 && (
            <div className="selection-bar" aria-live="polite">
              <strong>
                {selected.size} seleccionado{selected.size === 1 ? "" : "s"}
              </strong>
              <Select
                disabled={libraryDrop.isPending}
                onValueChange={(statusId) => {
                  const status = bootstrap?.statuses.find((item) => item.id === statusId);
                  if (status)
                    moveSelection({ kind: "status", id: status.id }, `estado ${status.name}`);
                }}
              >
                <SelectTrigger aria-label="Mover selección a un estado">
                  <SelectValue placeholder="Cambiar estado…" />
                </SelectTrigger>
                <SelectContent>
                  {bootstrap?.statuses.map((status) => (
                    <SelectItem key={status.id} value={status.id}>
                      {status.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select
                disabled={libraryDrop.isPending}
                onValueChange={(collectionId) => {
                  const collection = bootstrap?.collections.find(
                    (item) => item.id === collectionId && item.kind === "manual",
                  );
                  if (collection) {
                    moveSelection(
                      { kind: "collection", id: collection.id },
                      `colección ${collection.name}`,
                    );
                  }
                }}
              >
                <SelectTrigger aria-label="Añadir selección a una colección manual">
                  <SelectValue placeholder="Añadir a colección…" />
                </SelectTrigger>
                <SelectContent>
                  {bootstrap?.collections
                    .filter((collection) => collection.kind === "manual")
                    .map((collection) => (
                      <SelectItem key={collection.id} value={collection.id}>
                        {collection.name}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
              {plannerColumns.length > 0 && (
                <Select
                  disabled={addToPlanner.isPending}
                  onValueChange={(columnId) =>
                    addToPlanner.mutate({ appIds: [...selected], columnId })
                  }
                >
                  <SelectTrigger aria-label="Añadir selección al plan">
                    <SelectValue placeholder="Añadir al plan…" />
                  </SelectTrigger>
                  <SelectContent>
                    {plannerColumns.map((column) => (
                      <SelectItem key={column.id} value={column.id}>
                        {column.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
              {selected.size === 1 && (
                <Button size="sm" variant="secondary" onClick={() => setDetailId([...selected][0])}>
                  Abrir ficha
                </Button>
              )}
              <Button
                size="sm"
                variant="secondary"
                disabled={archiveSelection.isPending}
                onClick={() =>
                  archiveSelection.mutate({
                    appIds: [...selected],
                    archived: archiveScope !== "archived",
                  })
                }
              >
                {archiveScope === "archived" ? "Devolver a la biblioteca" : "Archivar"}
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Limpiar selección"
                onClick={() => setSelected(new Set())}
              >
                <IconX />
              </Button>
            </div>
          )}
          {operationMessage && visibleTotal > 0 && selected.size === 0 && (
            <div
              className="library-batch-feedback"
              data-error={libraryDrop.isError || undoDrop.isError}
              role={libraryDrop.isError || undoDrop.isError ? "alert" : "status"}
            >
              <span>{operationMessage}</span>
              {undoReceipt ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={undoDrop.isPending}
                  onClick={() => undoDrop.mutate(undoReceipt)}
                >
                  <IconArrowBackUp /> Deshacer
                </Button>
              ) : orderUndo ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={reorderCollections.isPending}
                  onClick={undoCollectionOrder}
                >
                  <IconArrowBackUp /> Deshacer
                </Button>
              ) : undoLabel ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={organizeGame.isPending}
                  onClick={undoLast}
                  title={undoLabel}
                >
                  <IconArrowBackUp /> Deshacer
                </Button>
              ) : null}
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label="Cerrar aviso"
                onClick={() => {
                  setOperationMessage(undefined);
                  setUndoReceipt(undefined);
                  setOrderUndo(undefined);
                }}
              >
                <IconX />
              </Button>
            </div>
          )}
        </section>
        {detailId && (
          <Suspense fallback={null}>
            <GameDetailSheet
              appId={detailId}
              open={Boolean(detailId)}
              onOpenChange={(open) => !open && setDetailId(undefined)}
              statuses={bootstrap?.statuses ?? []}
              collections={bootstrap?.collections ?? []}
              confirmUninstall={bootstrap?.preferences.confirmUninstall ?? true}
            />
          </Suspense>
        )}
        {collectionEditorOpen && (
          <Suspense fallback={null}>
            <CollectionEditorDialog
              open={collectionEditorOpen}
              onOpenChange={setCollectionEditorOpen}
              statuses={bootstrap?.statuses}
            />
          </Suspense>
        )}
        <AlertDialog
          open={Boolean(uninstallTarget)}
          onOpenChange={(open) => {
            if (!open) setUninstallTarget(undefined);
          }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>
                ¿Solicitar la desinstalación de {uninstallTarget?.title}?
              </AlertDialogTitle>
              <AlertDialogDescription>
                Vindexa no borrará archivos directamente. Abrirá el cliente oficial de Steam para
                que revises y completes la desinstalación.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancelar</AlertDialogCancel>
              <AlertDialogAction
                variant="destructive"
                onClick={() => {
                  const target = uninstallTarget;
                  setUninstallTarget(undefined);
                  if (!target) return;
                  runGameAction(
                    target,
                    `Solicitando la desinstalación de ${target.title}…`,
                    `Steam recibió la solicitud para desinstalar ${target.title}.`,
                    () => api.uninstallGame(target.appId),
                  );
                }}
              >
                Solicitar a Steam
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
      <DragOverlay dropAnimation={null}>
        {activeCollectionDrag ? (
          <div className="library-drag-overlay library-drag-overlay--collection">
            <IconGripVertical aria-hidden="true" />
            <div>
              <strong>Reordenar colección</strong>
              <span>{activeCollectionDrag.title}</span>
            </div>
          </div>
        ) : activeDrag ? (
          <div className="library-drag-overlay library-drag-overlay--game">
            <span className="library-drag-overlay__cover" aria-hidden="true">
              <Artwork
                appId={activeDrag.appId}
                src={activeDrag.coverUrl}
                title={activeDrag.title}
              />
              {activeDrag.appIds.length > 1 && (
                <em className="library-drag-overlay__count">{activeDrag.appIds.length}</em>
              )}
            </span>
            <div>
              <strong>
                {activeDrag.appIds.length === 1 ? activeDrag.title : "Selección múltiple"}
              </strong>
              <span>
                {activeDrag.appIds.length} juego{activeDrag.appIds.length === 1 ? "" : "s"} · suelta
                en un estado o colección
              </span>
            </div>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

function LibrarySkeleton() {
  const placeholders = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o"];
  return (
    <div className="library-skeleton" aria-label="Cargando juegos" role="status">
      {placeholders.map((placeholder) => (
        <div key={placeholder}>
          <span />
          <i />
          <i />
        </div>
      ))}
    </div>
  );
}
