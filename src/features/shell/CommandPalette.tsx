import {
  IconBaselineDensityMedium,
  IconBooks,
  IconBrandSteam,
  IconChecklist,
  IconCopy,
  IconDatabase,
  IconDeviceGamepad2,
  IconDownload,
  IconEraser,
  IconEye,
  IconEyeOff,
  IconFlag,
  IconFolderOpen,
  IconFolderPlus,
  IconFolders,
  IconHash,
  IconHeart,
  IconInfoCircle,
  IconLayoutGrid,
  IconLayoutKanban,
  IconLayoutList,
  IconLayoutRows,
  IconListNumbers,
  IconPin,
  IconPinnedOff,
  IconPlayerPlay,
  IconRefresh,
  IconSettings,
  IconTrash,
} from "@tabler/icons-react";
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useDeferredValue, useEffect, useMemo, useState } from "react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import {
  fuzzyScore,
  GAME_RESULT_LIMIT,
  mergeGameResults,
  normalizeSearchText,
  rankGames,
} from "@/features/shell/command-ranking";
import type { InterfaceDensity } from "@/features/shell/interface-density";
import {
  dispatchLibraryCommand,
  type LibraryContextSnapshot,
  type ShortcutMap,
  sectionShortcut,
  shortcutLabel,
} from "@/features/shell/shortcuts";
import { formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppSection, GameSummary, LibraryView } from "@/lib/types";
import "./command-palette.css";

/**
 * Paleta de comandos de Vindexa.
 *
 * Toma prestado de Raycast el gesto y **sus dos mitades**: `Intro` ejecuta la
 * acción principal sobre el juego enfocado y `Mod+K` abre este panel con todas
 * las acciones contextuales, buscables con coincidencia difusa. La biblioteca
 * puede tener miles de entradas, así que el filtrado interno de `cmdk` está
 * desactivado (`shouldFilter={false}`): puntuar cada juego renderizando un
 * `CommandItem` por título haría 5.000 nodos por pulsación. En su lugar se
 * puntúa sobre datos, con el texto normalizado en caché y una selección de los
 * mejores K resultados en una sola pasada sin ordenar la lista entera.
 *
 * La página cargada es sólo una ventana sobre SQLite, de modo que buscar en ella
 * no basta: a partir de dos caracteres se pregunta también al catálogo completo
 * y sus resultados se añaden **debajo** de los locales, sin estados de carga que
 * hagan saltar la lista y sin romper nada si la llamada falla.
 */

/** Longitud a partir de la cual merece la pena preguntar al catálogo entero. */
const CATALOG_MIN_QUERY = 2;
/** Retardo antes de consultar SQLite: escribir no debe disparar una consulta. */
const CATALOG_DEBOUNCE_MS = 180;
/** Filas del catálogo que se añaden bajo los resultados de la página cargada. */
const CATALOG_RESULT_LIMIT = 4;
/** Candidatos que se piden a SQLite antes de reordenarlos por título. */
const CATALOG_FETCH_LIMIT = 40;

type PaletteIcon = typeof IconBooks;

interface PaletteEntry {
  id: string;
  label: string;
  /** Texto adicional que también se busca, pero no se muestra como título. */
  keywords: string;
  detail?: string | undefined;
  shortcut?: string | undefined;
  icon: PaletteIcon;
  destructive?: boolean | undefined;
  /** La acción cierra la paleta cuando termina, no al pulsarla. */
  deferredClose?: boolean | undefined;
  run: () => void;
}

interface PaletteGroup {
  id: string;
  heading: string;
  entries: PaletteEntry[];
}

/** Operación en curso o fallida; ocupa el hueco del recuento en el pie. */
interface PaletteOperation {
  message: string;
  failed: boolean;
}

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  bindings: ShortcutMap;
  section: AppSection;
  density: InterfaceDensity;
  context?: LibraryContextSnapshot | undefined;
  /** Consulta con la que arranca el panel; la usa «Añadir a una colección». */
  initialQuery?: string | undefined;
  onNavigate: (section: AppSection) => void;
  onOpenSettings: () => void;
  onSync: () => void;
  onSetDensity: (density: InterfaceDensity) => void;
  onClearArtCache: () => void;
  onMaintainArtCache: () => void;
}

const SECTIONS: readonly { id: AppSection; label: string; icon: PaletteIcon }[] = [
  { id: "library", label: "Biblioteca", icon: IconBooks },
  { id: "planner", label: "Planificador", icon: IconLayoutKanban },
  { id: "wishlist", label: "Deseados", icon: IconHeart },
  { id: "collections", label: "Colecciones", icon: IconFolders },
  { id: "tracking", label: "Seguimiento", icon: IconChecklist },
  { id: "couch", label: "Modo sofá", icon: IconDeviceGamepad2 },
];

const VIEWS: readonly { id: LibraryView; label: string; icon: PaletteIcon }[] = [
  { id: "grid", label: "Cuadrícula", icon: IconLayoutGrid },
  { id: "list", label: "Lista", icon: IconLayoutList },
  { id: "compact", label: "Compacta", icon: IconLayoutRows },
];

const PRIORITIES: readonly number[] = [0, 1, 2, 3, 4, 5];

/** Referencias estables para que los `useMemo` no se invaliden sin contexto. */
const NO_GAMES: readonly GameSummary[] = [];
const NO_COLUMNS: readonly { id: string; name: string }[] = [];

function priorityLabel(priority: number): string {
  return priority === 0 ? "Sin prioridad" : `Prioridad ${priority}`;
}

/** «12 juegos», «1 juego»: el recuento va siempre en el texto de la acción. */
function gameCount(count: number): string {
  return `${count} juego${count === 1 ? "" : "s"}`;
}

export function CommandPalette({
  open,
  onOpenChange,
  bindings,
  section,
  density,
  context,
  initialQuery,
  onNavigate,
  onOpenSettings,
  onSync,
  onSetDensity,
  onClearArtCache,
  onMaintainArtCache,
}: CommandPaletteProps) {
  const queryClient = useQueryClient();
  const [query, setQuery] = useState(initialQuery ?? "");
  // La consulta diferida deja escribir a velocidad de teclado aunque la lista
  // de resultados tarde un fotograma más en alcanzarla.
  const deferredQuery = useDeferredValue(query);
  const [operation, setOperation] = useState<PaletteOperation>();
  useEffect(() => {
    if (!open) return;
    setQuery(initialQuery ?? "");
    setOperation(undefined);
  }, [open, initialQuery]);

  // El catálogo se consulta con retardo y sólo desde dos caracteres: escribir
  // no debe disparar una consulta por pulsación.
  const [catalogQuery, setCatalogQuery] = useState("");
  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < CATALOG_MIN_QUERY) {
      setCatalogQuery("");
      return;
    }
    const timer = window.setTimeout(() => setCatalogQuery(trimmed), CATALOG_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  /**
   * Búsqueda en todo SQLite, no sólo en la página cargada. Sin reintentos y sin
   * leer el error: fuera de Tauri —o si la consulta falla— la paleta se queda
   * con lo local en silencio, que es exactamente lo que había antes.
   */
  const catalogSearch = useQuery({
    queryKey: ["palette-catalog", catalogQuery],
    queryFn: () => api.listGames({ query: catalogQuery, limit: CATALOG_FETCH_LIMIT }),
    enabled: catalogQuery.length >= CATALOG_MIN_QUERY,
    retry: false,
    staleTime: 60_000,
    // Mantener la respuesta anterior mientras llega la nueva evita que las filas
    // del catálogo desaparezcan y vuelvan a aparecer en cada pulsación.
    placeholderData: keepPreviousData,
  });

  const games = context?.games ?? NO_GAMES;
  // Si la biblioteca todavía no publicó sus columnas, la paleta se limita a no
  // ofrecer el plan en lugar de inventarse un destino.
  const plannerColumns = context?.plannerColumns ?? NO_COLUMNS;
  const focusedGame = useMemo(
    () =>
      context?.focusedAppId !== undefined
        ? games.find((game) => game.appId === context.focusedAppId)
        : undefined,
    [context?.focusedAppId, games],
  );
  const selectedGames = useMemo(() => {
    const ids = context?.selectedAppIds ?? [];
    if (ids.length < 2) return NO_GAMES;
    const byId = new Map(games.map((game) => [game.appId, game]));
    return ids
      .map((appId) => byId.get(appId))
      .filter((game): game is GameSummary => game !== undefined);
  }, [context?.selectedAppIds, games]);

  const close = () => onOpenChange(false);
  const perform = (action: () => void) => {
    action();
    close();
  };

  /**
   * Ejecuta una acción masiva que habla con SQLite. La paleta se queda abierta
   * hasta que termina: si falla, el motivo se lee en el pie en lugar de
   * perderse al cerrarse el panel.
   */
  const runOperation = useCallback(
    (pending: string, task: () => Promise<unknown>) => {
      setOperation({ message: pending, failed: false });
      void task()
        .then(() => {
          setOperation(undefined);
          onOpenChange(false);
        })
        .catch((cause: unknown) => setOperation({ message: getErrorMessage(cause), failed: true }));
    },
    [onOpenChange],
  );

  const planEntries = useCallback(
    (appIds: readonly number[], actionLabel: string): PaletteEntry[] =>
      plannerColumns.map((column) => ({
        id: `plan:${column.id}`,
        label: `${actionLabel} · ${column.name}`,
        keywords: `plan planificador tablero columna objetivo backlog ${column.name}`,
        icon: IconLayoutKanban,
        run: () => dispatchLibraryCommand({ kind: "addToPlanner", appIds, columnId: column.id }),
      })),
    [plannerColumns],
  );

  const selectionGroup = useMemo<PaletteGroup | undefined>(() => {
    if (selectedGames.length < 2) return undefined;
    const appIds = selectedGames.map((game) => game.appId);
    const describe = gameCount(appIds.length);
    const entries: PaletteEntry[] = [];
    const applyDrop = (target: { kind: "status" | "collection"; id: string }, pending: string) =>
      runOperation(pending, () =>
        api.applyLibraryDrop({ appIds, target }).then(() => queryClient.invalidateQueries()),
      );
    for (const status of context?.statuses ?? []) {
      entries.push({
        id: `selection:status:${status.id}`,
        label: `Mover ${describe} a «${status.name}»`,
        keywords: `estado masivo seleccion ${status.name}`,
        icon: IconFlag,
        deferredClose: true,
        run: () =>
          applyDrop({ kind: "status", id: status.id }, `Moviendo ${describe} a ${status.name}…`),
      });
    }
    for (const collection of context?.collections ?? []) {
      if (collection.kind !== "manual") continue;
      entries.push({
        id: `selection:collection:${collection.id}`,
        label: `Añadir ${describe} a la colección «${collection.name}»`,
        keywords: `coleccion masivo seleccion ${collection.name}`,
        icon: IconFolderPlus,
        deferredClose: true,
        run: () =>
          applyDrop(
            { kind: "collection", id: collection.id },
            `Añadiendo ${describe} a ${collection.name}…`,
          ),
      });
    }
    // Fijar y seguir son conmutadores: sobre una selección mixta se anuncia y se
    // aplica sólo el cambio que falta, para que el recuento del texto sea el que
    // de verdad se va a mover.
    const toPin = selectedGames.filter((game) => !game.pinned);
    const pinTargets = toPin.length ? toPin : selectedGames;
    entries.push({
      id: "selection:pin",
      label: toPin.length
        ? `Fijar ${gameCount(toPin.length)} en la biblioteca`
        : `Desfijar ${describe} de la biblioteca`,
      keywords: "fijar anclar destacar pin masivo seleccion",
      icon: toPin.length ? IconPin : IconPinnedOff,
      run: () => {
        for (const game of pinTargets) {
          dispatchLibraryCommand({ kind: "togglePinned", appId: game.appId });
        }
      },
    });
    const toTrack = selectedGames.filter((game) => !game.tracking);
    const trackTargets = toTrack.length ? toTrack : selectedGames;
    entries.push({
      id: "selection:tracking",
      label: toTrack.length
        ? `Marcar seguimiento en ${gameCount(toTrack.length)}`
        : `Quitar de seguimiento ${describe}`,
      keywords: "seguimiento seguir vigilar tracking masivo seleccion",
      icon: toTrack.length ? IconEye : IconEyeOff,
      run: () => {
        for (const game of trackTargets) {
          dispatchLibraryCommand({ kind: "toggleTracking", appId: game.appId });
        }
      },
    });
    entries.push(...planEntries(appIds, `Añadir ${describe} al plan`));
    return { id: "selection", heading: `${describe} seleccionados`, entries };
  }, [
    context?.collections,
    context?.statuses,
    planEntries,
    queryClient,
    runOperation,
    selectedGames,
  ]);

  const gameGroup = useMemo<PaletteGroup | undefined>(() => {
    if (!focusedGame) return undefined;
    const game = focusedGame;
    const appId = game.appId;
    const entries: PaletteEntry[] = [];
    entries.push(
      game.installed
        ? {
            id: "game:play",
            label: `Jugar a ${game.title}`,
            keywords: "iniciar lanzar ejecutar steam",
            icon: IconPlayerPlay,
            shortcut: bindings.primaryAction,
            run: () => dispatchLibraryCommand({ kind: "play", appId }),
          }
        : {
            id: "game:install",
            label: `Instalar ${game.title}`,
            keywords: "descargar steam bajar",
            icon: IconDownload,
            shortcut: bindings.primaryAction,
            run: () => dispatchLibraryCommand({ kind: "install", appId }),
          },
    );
    entries.push({
      id: "game:detail",
      label: "Abrir la ficha",
      keywords: "detalle informacion ficha panel",
      icon: IconInfoCircle,
      shortcut: bindings.openDetail,
      run: () => dispatchLibraryCommand({ kind: "openDetail", appId }),
    });
    entries.push({
      id: "game:store",
      label: "Abrir la tienda integrada",
      keywords: "steam store tienda pagina oficial",
      icon: IconBrandSteam,
      shortcut: bindings.openStore,
      run: () => dispatchLibraryCommand({ kind: "openStore", appId }),
    });
    if (game.installed) {
      entries.push({
        id: "game:reveal",
        label: "Revelar la carpeta de instalación",
        keywords: "carpeta archivos disco ruta explorador finder",
        icon: IconFolderOpen,
        shortcut: bindings.revealInstallation,
        run: () => dispatchLibraryCommand({ kind: "reveal", appId }),
      });
    }
    // El backlog se descubre aquí, así que aquí se planifica: sin esta entrada
    // habría que abandonar la biblioteca y recordar el título de memoria.
    entries.push(...planEntries([appId], "Añadir al plan"));
    for (const status of context?.statuses ?? []) {
      if (status.id === game.statusId) continue;
      entries.push({
        id: `game:status:${status.id}`,
        label: `Cambiar el estado a «${status.name}»`,
        keywords: `estado ${status.name}`,
        icon: IconFlag,
        run: () => dispatchLibraryCommand({ kind: "setStatus", appId, statusId: status.id }),
      });
    }
    for (const priority of PRIORITIES) {
      if (priority === game.priority) continue;
      entries.push({
        id: `game:priority:${priority}`,
        label: `Ajustar a ${priorityLabel(priority).toLowerCase()}`,
        keywords: `prioridad ${priority} orden importancia`,
        icon: IconListNumbers,
        run: () => dispatchLibraryCommand({ kind: "setPriority", appId, priority }),
      });
    }
    entries.push({
      id: "game:pin",
      label: game.pinned ? "Desfijar de la biblioteca" : "Fijar en la biblioteca",
      keywords: "fijar anclar destacar pin",
      icon: game.pinned ? IconPinnedOff : IconPin,
      shortcut: bindings.togglePinned,
      run: () => dispatchLibraryCommand({ kind: "togglePinned", appId }),
    });
    entries.push({
      id: "game:tracking",
      label: game.tracking ? "Quitar de seguimiento" : "Marcar seguimiento",
      keywords: "seguimiento seguir vigilar tracking",
      icon: game.tracking ? IconEyeOff : IconEye,
      shortcut: bindings.toggleTracking,
      run: () => dispatchLibraryCommand({ kind: "toggleTracking", appId }),
    });
    const memberships = new Set(
      context?.collectionIdsByApp?.get(appId) ?? game.collectionIds ?? [],
    );
    for (const collection of context?.collections ?? []) {
      if (collection.kind !== "manual") continue;
      const member = memberships.has(collection.id);
      entries.push({
        id: `game:collection:${collection.id}`,
        label: member
          ? `Quitar de la colección «${collection.name}»`
          : `Añadir a la colección «${collection.name}»`,
        keywords: `coleccion ${collection.name}`,
        icon: IconFolderPlus,
        run: () =>
          dispatchLibraryCommand({ kind: "toggleCollection", appId, collectionId: collection.id }),
      });
    }
    entries.push({
      id: "game:copy-title",
      label: "Copiar el título",
      keywords: "copiar portapapeles nombre",
      icon: IconCopy,
      shortcut: bindings.copyTitle,
      run: () => dispatchLibraryCommand({ kind: "copyTitle", appId }),
    });
    entries.push({
      id: "game:copy-appid",
      label: "Copiar el AppID",
      keywords: `copiar portapapeles identificador ${appId}`,
      icon: IconHash,
      shortcut: bindings.copyAppId,
      run: () => dispatchLibraryCommand({ kind: "copyAppId", appId }),
    });
    if (game.installed) {
      entries.push({
        id: "game:uninstall",
        label: "Solicitar la desinstalación",
        keywords: "desinstalar borrar liberar espacio",
        icon: IconTrash,
        destructive: true,
        shortcut: bindings.requestUninstall,
        run: () => dispatchLibraryCommand({ kind: "uninstall", appId }),
      });
    }
    return { id: "game", heading: `Juego enfocado · ${game.title}`, entries };
  }, [bindings, context, focusedGame, planEntries]);

  const globalGroups = useMemo<PaletteGroup[]>(() => {
    const navigation: PaletteEntry[] = SECTIONS.map(({ id, label, icon }) => ({
      id: `section:${id}`,
      label: `Ir a ${label}`,
      keywords: `seccion navegar ${label}`,
      icon,
      // Una sección sin combinación asignada aparece igual, sólo que sin atajo.
      shortcut: sectionShortcut(bindings, id),
      ...(section === id ? { detail: "Sección actual" } : {}),
      run: () => onNavigate(id),
    }));
    const application: PaletteEntry[] = [
      {
        id: "app:sync",
        label: "Sincronizar con Steam",
        keywords: "steam sincronizar actualizar biblioteca importar",
        icon: IconRefresh,
        shortcut: bindings.sync,
        run: onSync,
      },
      {
        id: "app:settings",
        label: "Abrir los ajustes",
        keywords: "ajustes preferencias configuracion opciones",
        icon: IconSettings,
        shortcut: "Mod+Comma",
        run: onOpenSettings,
      },
      ...VIEWS.map<PaletteEntry>(({ id, label, icon }) => ({
        id: `app:view:${id}`,
        label: `Ver la biblioteca en ${label.toLowerCase()}`,
        keywords: `vista ${label} disposicion`,
        icon,
        ...(context?.view === id ? { detail: "Vista actual" } : {}),
        run: () => {
          onNavigate("library");
          dispatchLibraryCommand({ kind: "setView", view: id });
        },
      })),
      {
        id: "app:density:compact",
        label: "Usar la densidad compacta",
        keywords: "densidad compacta altura filas",
        icon: IconBaselineDensityMedium,
        ...(density === "compact" ? { detail: "Densidad actual" } : {}),
        run: () => onSetDensity("compact"),
      },
      {
        id: "app:density:comfortable",
        label: "Usar la densidad cómoda",
        keywords: "densidad comoda altura filas espaciada",
        icon: IconBaselineDensityMedium,
        ...(density === "comfortable" ? { detail: "Densidad actual" } : {}),
        run: () => onSetDensity("comfortable"),
      },
      {
        id: "app:art-maintain",
        label: "Depurar la caché de arte",
        keywords: "cache arte portadas mantenimiento depurar limpiar huerfanos",
        icon: IconDatabase,
        run: onMaintainArtCache,
      },
      {
        id: "app:art-clear",
        label: "Vaciar la caché de arte",
        keywords: "cache arte portadas vaciar borrar imagenes",
        icon: IconEraser,
        destructive: true,
        run: onClearArtCache,
      },
    ];
    return [
      { id: "navigation", heading: "Ir a", entries: navigation },
      { id: "application", heading: "Aplicación", entries: application },
    ];
  }, [
    bindings,
    context?.view,
    density,
    onClearArtCache,
    onMaintainArtCache,
    onNavigate,
    onOpenSettings,
    onSetDensity,
    onSync,
    section,
  ]);

  const actionGroups = useMemo<PaletteGroup[]>(() => {
    const contextual = selectionGroup ?? gameGroup;
    const source = contextual ? [contextual, ...globalGroups] : globalGroups;
    const needle = normalizeSearchText(deferredQuery.trim());
    if (!needle) return source;
    return source
      .map((group) => ({
        ...group,
        entries: group.entries
          .map((entry) => ({
            entry,
            score: fuzzyScore(normalizeSearchText(`${entry.label} ${entry.keywords}`), needle),
          }))
          .filter((candidate) => candidate.score >= 0)
          .sort((left, right) => right.score - left.score)
          .map((candidate) => candidate.entry),
      }))
      .filter((group) => group.entries.length > 0);
  }, [deferredQuery, gameGroup, globalGroups, selectionGroup]);

  const gamesGroup = useMemo<PaletteGroup | undefined>(() => {
    const local = rankGames(games, deferredQuery, GAME_RESULT_LIMIT);
    // Sin consulta viva al catálogo no se mezcla nada: si no, al borrar el texto
    // sobrevivirían las filas de la búsqueda anterior.
    const catalog =
      catalogQuery.length >= CATALOG_MIN_QUERY ? (catalogSearch.data?.items ?? NO_GAMES) : NO_GAMES;
    const results = mergeGameResults(local, catalog, deferredQuery, CATALOG_RESULT_LIMIT);
    if (!results.length) return undefined;
    return {
      id: "games",
      heading: "Juegos",
      entries: results.map(({ game, fromCatalog }) => ({
        id: `game-result:${game.appId}`,
        label: game.title,
        keywords: "",
        icon: IconDeviceGamepad2,
        detail: fromCatalog
          ? `${game.statusName} · Catálogo completo`
          : `${game.statusName} · ${formatPlaytime(game.playtimeMinutes)}${game.installed ? " · Instalado" : ""}`,
        run: () => {
          if (section !== "library") onNavigate("library");
          dispatchLibraryCommand({ kind: "focus", appId: game.appId });
          dispatchLibraryCommand({ kind: "openDetail", appId: game.appId });
        },
      })),
    };
  }, [catalogQuery, catalogSearch.data, deferredQuery, games, onNavigate, section]);

  /**
   * Orden de lectura: primero lo que se puede hacer con el juego enfocado o con
   * la selección —igual que en Raycast, la acción principal es la primera y
   * `Intro` la ejecuta—, después los juegos que coinciden y por último la
   * aplicación.
   */
  const visibleGroups = useMemo(() => {
    const groups = [...actionGroups];
    if (gamesGroup) {
      const contextual = groups.findIndex(
        (group) => group.id === "game" || group.id === "selection",
      );
      groups.splice(contextual + 1, 0, gamesGroup);
    }
    return groups;
  }, [actionGroups, gamesGroup]);

  const resultCount = visibleGroups.reduce((total, group) => total + group.entries.length, 0);
  const trimmedQuery = deferredQuery.trim();

  if (!open) return null;

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Paleta de comandos"
      description="Busca acciones sobre el juego enfocado, juegos de la biblioteca y secciones de Vindexa."
      className="command-palette rounded-none!"
    >
      <Command
        shouldFilter={false}
        label="Paleta de comandos"
        className="command-palette__surface rounded-none! bg-transparent p-0"
      >
        <CommandInput
          value={query}
          onValueChange={setQuery}
          placeholder="Busca una acción, un juego o una sección…"
        />
        <CommandList className="command-palette__list">
          <CommandEmpty className="command-palette__empty">
            <strong>Nada coincide con «{trimmedQuery}».</strong>
            <span>Prueba con el título de un juego.</span>
          </CommandEmpty>
          {visibleGroups.map((group) => (
            <CommandGroup key={group.id} heading={group.heading} className="command-palette__group">
              {group.entries.map((entry) => (
                <CommandItem
                  key={entry.id}
                  value={entry.id}
                  className="command-palette__item"
                  data-destructive={entry.destructive ? "true" : undefined}
                  onSelect={() => (entry.deferredClose ? entry.run() : perform(entry.run))}
                >
                  <entry.icon aria-hidden="true" />
                  <span className="command-palette__label">{entry.label}</span>
                  {entry.detail && <span className="command-palette__detail">{entry.detail}</span>}
                  {entry.shortcut && (
                    <CommandShortcut className="command-palette__shortcut">
                      {shortcutLabel(entry.shortcut)}
                    </CommandShortcut>
                  )}
                </CommandItem>
              ))}
            </CommandGroup>
          ))}
        </CommandList>
        <footer className="command-palette__footer">
          {operation ? (
            <span role={operation.failed ? "alert" : "status"}>{operation.message}</span>
          ) : (
            <span>
              {resultCount} resultado{resultCount === 1 ? "" : "s"}
            </span>
          )}
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> moverse · <kbd>{shortcutLabel("Enter")}</kbd> ejecutar ·{" "}
            <kbd>{shortcutLabel("Escape")}</kbd> cerrar
          </span>
        </footer>
      </Command>
    </CommandDialog>
  );
}
