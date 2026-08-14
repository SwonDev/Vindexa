import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
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
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FamilyCatalogBrowser } from "@/features/library/FamilyCatalogBrowser";
import { GameBrowser } from "@/features/library/GameBrowser";
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
import {
  libraryScopeKey,
  readLibraryScroll,
  readLibrarySession,
  writeLibraryScroll,
  writeLibrarySession,
} from "@/features/library/library-session";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  AppBootstrap,
  GameListRequest,
  GameSort,
  GameSummary,
  LibraryDropReceipt,
  LibraryDropTarget,
  LibraryView,
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
  const [filters, setFilters] = useState<ExtraFilters>(session.filters);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [detailId, setDetailId] = useState<number>();
  const [collectionEditorOpen, setCollectionEditorOpen] = useState(false);
  const [operationMessage, setOperationMessage] = useState<string>();
  const [undoReceipt, setUndoReceipt] = useState<LibraryDropReceipt>();
  const [activeDrag, setActiveDrag] = useState<{ appIds: number[]; title: string }>();
  const [activeCollectionDrag, setActiveCollectionDrag] = useState<{
    id: string;
    title: string;
  }>();
  const [collectionOrder, setCollectionOrder] = useState<string[]>([]);
  const awaitingInitialPreferences = useRef(!bootstrap);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  useEffect(() => {
    writeLibrarySession({ scope, query, sort, randomSeed, view, filters });
  }, [filters, query, randomSeed, scope, sort, view]);
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
      sort,
      ...(sort === "random" ? { sortSeed: randomSeed } : {}),
      limit: 240,
    };
  }, [debouncedQuery, filters, randomSeed, scope, sort]);
  const gamesQuery = useInfiniteQuery({
    queryKey: ["games", requestBase],
    queryFn: ({ pageParam }) => api.listGames({ ...requestBase, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (last) =>
      last.offset + last.items.length < last.total ? last.offset + last.items.length : undefined,
    enabled: scope.kind !== "family",
  });
  const familyQuery = useInfiniteQuery({
    queryKey: ["family-catalog", debouncedQuery],
    queryFn: ({ pageParam }) =>
      api.listFamilyCatalog({
        ...(debouncedQuery.trim() ? { query: debouncedQuery.trim() } : {}),
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
  const activeFilters = Boolean(
    debouncedQuery ||
      activeLibraryFilterCount(filters) ||
      (scope.kind !== "all" && scope.kind !== "family"),
  );

  useEffect(() => {
    const focusSearch = () => {
      document.querySelector<HTMLInputElement>(".search-field input")?.focus();
    };
    const closePanel = () => {
      if (detailId) setDetailId(undefined);
      else if (collectionEditorOpen) setCollectionEditorOpen(false);
      else if (selected.size) setSelected(new Set());
    };
    window.addEventListener("vindexa:focus-search", focusSearch);
    window.addEventListener("vindexa:close-panel", closePanel);
    return () => {
      window.removeEventListener("vindexa:focus-search", focusSearch);
      window.removeEventListener("vindexa:close-panel", closePanel);
    };
  }, [collectionEditorOpen, detailId, selected.size]);

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
  const changeSort = (nextSort: GameSort) => {
    setSort(nextSort);
    if (nextSort === "random") {
      setRandomSeed(crypto.getRandomValues(new Uint32Array(1))[0] ?? 0);
    }
    if (bootstrap) {
      sortPreference.mutate({ ...bootstrap.preferences, librarySort: nextSort });
    }
  };
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
    if (!selected.has(appId)) setSelected(new Set([appId]));
    setActiveDrag({ appIds, title });
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
      reorderCollections.mutate({ previous, next });
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
  const selectGame = (game: GameSummary, additive: boolean) => {
    setSelected((current) => {
      if (!additive) return new Set([game.appId]);
      const next = new Set(current);
      if (next.has(game.appId)) next.delete(game.appId);
      else next.add(game.appId);
      return next;
    });
  };

  if (loading && !bootstrap)
    return (
      <div className="library-layout">
        <LibrarySidebar
          bootstrap={sidebarBootstrap}
          scope={scope}
          onScopeChange={changeScope}
          onCreateCollection={() => setCollectionEditorOpen(true)}
          familyCount={familyQuery.data ? familyTotal : undefined}
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
          familyCount={familyQuery.data ? familyTotal : undefined}
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
      collisionDetection={closestCenter}
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
          familyCount={familyQuery.data ? familyTotal : undefined}
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
            filters={filters}
            onFiltersChange={setFilters}
            statuses={bootstrap?.statuses ?? []}
            collections={bootstrap?.collections ?? []}
            filterOptions={filterOptionsQuery.data}
          />
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
                    setFilters({});
                    setScope({ kind: "all", label: "Todos los juegos" });
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
              hasMore={Boolean(familyQuery.hasNextPage)}
              loadingMore={familyQuery.isFetchingNextPage}
              initialScrollOffset={readLibraryScroll(scope)}
              onScrollOffsetChange={(offset) => writeLibraryScroll(scope, offset)}
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
              focusedGameId={detailId}
              hasMore={Boolean(gamesQuery.hasNextPage)}
              loadingMore={gamesQuery.isFetchingNextPage}
              initialScrollOffset={readLibraryScroll(scope)}
              onScrollOffsetChange={(offset) => writeLibraryScroll(scope, offset)}
              onLoadMore={() => gamesQuery.fetchNextPage()}
              onSelect={selectGame}
              onOpen={(game) => setDetailId(game.appId)}
              manualPositioning={sort === "manual" && scope.kind === "all"}
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
          {selected.size > 0 && (
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
              {selected.size === 1 && (
                <Button size="sm" variant="secondary" onClick={() => setDetailId([...selected][0])}>
                  Abrir ficha
                </Button>
              )}
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
              {undoReceipt && (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={undoDrop.isPending}
                  onClick={() => undoDrop.mutate(undoReceipt)}
                >
                  <IconArrowBackUp /> Deshacer
                </Button>
              )}
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label="Cerrar aviso"
                onClick={() => {
                  setOperationMessage(undefined);
                  setUndoReceipt(undefined);
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
          <div className="library-drag-overlay">
            <IconGripVertical aria-hidden="true" />
            <div>
              <strong>
                {activeDrag.appIds.length} juego{activeDrag.appIds.length === 1 ? "" : "s"}
              </strong>
              <span>
                {activeDrag.appIds.length === 1 ? activeDrag.title : "Selección múltiple"}
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
