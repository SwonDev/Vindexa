import {
  IconBooks,
  IconBrandSteam,
  IconChecklist,
  IconCommand,
  IconDeviceGamepad2,
  IconFolders,
  IconHeartPlus,
  IconLayoutKanban,
  IconRefresh,
  IconSettings,
} from "@tabler/icons-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { LoadingState } from "@/components/common/LoadingState";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { LibraryScreen } from "@/features/library/LibraryScreen";
import { NotificationsPopover } from "@/features/notifications/NotificationsPopover";
import { type InterfaceDensity, InterfaceDensityContext } from "@/features/shell/interface-density";
import {
  CLOSE_PANEL_EVENT,
  dispatchLibraryCommand,
  FOCUS_SEARCH_EVENT,
  hasTextSelection,
  isEditableShortcutTarget,
  isLibrarySurfaceTarget,
  type LibraryContextSnapshot,
  onLibraryContext,
  readLocalShortcuts,
  requestLibraryContext,
  requiresLibrarySurface,
  resolveShortcutEvent,
  resolveShortcuts,
  SHORTCUTS_CHANGED_EVENT,
  shortcutLabel,
} from "@/features/shell/shortcuts";
import { handleTitleBarPointerDown } from "@/features/shell/window-chrome";
import { invalidateSteamDerivedQueries } from "@/lib/steam-data-invalidation";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppSection } from "@/lib/types";

const PlannerScreen = lazy(() =>
  import("@/features/planner/PlannerScreen").then((module) => ({ default: module.PlannerScreen })),
);
const CollectionsScreen = lazy(() =>
  import("@/features/collections/CollectionsScreen").then((module) => ({
    default: module.CollectionsScreen,
  })),
);
const DiscoveryScreen = lazy(() =>
  import("@/features/discovery/DiscoveryScreen").then((module) => ({
    default: module.DiscoveryScreen,
  })),
);
const SettingsDialog = lazy(() =>
  import("@/features/settings/SettingsDialog").then((module) => ({
    default: module.SettingsDialog,
  })),
);
const CommandPalette = lazy(() =>
  import("@/features/shell/CommandPalette").then((module) => ({
    default: module.CommandPalette,
  })),
);

const WishlistScreen = lazy(() =>
  import("@/features/wishlist/WishlistScreen").then((module) => ({
    default: module.WishlistScreen,
  })),
);

const CouchScreen = lazy(() =>
  import("@/features/couch/CouchScreen").then((module) => ({
    default: module.CouchScreen,
  })),
);

/**
 * Orden de las secciones. Coincide con el número del atajo (`Mod+1`…`Mod+5`):
 * si aquí cambia el orden, el atajo de esa posición deja de corresponderse con
 * lo que la persona ve y hay que mover también su enlace.
 */
const sections = [
  { id: "library", label: "Biblioteca", icon: IconBooks },
  { id: "planner", label: "Planificador", icon: IconLayoutKanban },
  { id: "collections", label: "Colecciones", icon: IconFolders },
  { id: "tracking", label: "Seguimiento", icon: IconChecklist },
  { id: "wishlist", label: "Deseados", icon: IconHeartPlus },
] satisfies { id: AppSection; label: string; icon: typeof IconBooks }[];

export function AppShell() {
  const [section, setSection] = useState<AppSection>("library");
  const titleBarRef = useRef<HTMLElement>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteContext, setPaletteContext] = useState<LibraryContextSnapshot>();
  const [syncAnnouncement, setSyncAnnouncement] = useState("");
  const syncRunning = useRef(false);
  const queryClient = useQueryClient();
  const bootstrapQuery = useQuery({ queryKey: ["bootstrap"], queryFn: api.bootstrap });
  const bootstrap = bootstrapQuery.data;
  const metadataQuery = useQuery({
    queryKey: ["metadata-enrichment"],
    queryFn: api.metadataEnrichmentStatus,
    enabled: Boolean(bootstrap),
    refetchInterval: 5_000,
  });
  const metadata = metadataQuery.data;
  const metadataTerminalCount =
    (metadata?.succeeded ?? 0) + (metadata?.unavailable ?? 0) + (metadata?.failed ?? 0);
  const previousMetadataTerminalCount = useRef(0);
  useEffect(() => {
    if (metadataTerminalCount <= previousMetadataTerminalCount.current) return;
    previousMetadataTerminalCount.current = metadataTerminalCount;
    void queryClient.invalidateQueries({ queryKey: ["library-filter-options"] });
    void queryClient.invalidateQueries({ queryKey: ["games"] });
  }, [metadataTerminalCount, queryClient]);

  useEffect(() => {
    const titleBar = titleBarRef.current;
    if (!titleBar) return;
    const onMouseDown = (event: MouseEvent) => handleTitleBarPointerDown(event);
    titleBar.addEventListener("mousedown", onMouseDown);
    return () => titleBar.removeEventListener("mousedown", onMouseDown);
  }, []);

  /**
   * Atajos operativos. Los siete de navegación siguen viviendo en SQLite; los
   * que actúan sobre la biblioteca y sobre el juego enfocado se guardan en el
   * navegador hasta que el esquema de Rust los admita.
   */
  const [localShortcuts, setLocalShortcuts] = useState(readLocalShortcuts);
  useEffect(() => {
    const listener = () => setLocalShortcuts(readLocalShortcuts());
    window.addEventListener(SHORTCUTS_CHANGED_EVENT, listener);
    return () => window.removeEventListener(SHORTCUTS_CHANGED_EVENT, listener);
  }, []);
  const shortcuts = useMemo(
    () => resolveShortcuts(bootstrap?.preferences.shortcuts, localShortcuts),
    [bootstrap?.preferences.shortcuts, localShortcuts],
  );

  /**
   * Contexto de la biblioteca. Vive en una referencia y no en el estado a
   * propósito: cambia cada vez que entra una página de juegos, y guardarlo en
   * el estado repintaría el shell entero —y con él la rejilla virtualizada— en
   * cada desplazamiento. Sólo se copia al estado mientras la paleta está
   * abierta, que es cuando alguien lo está mirando.
   */
  const libraryContextRef = useRef<LibraryContextSnapshot | undefined>(undefined);
  const paletteOpenRef = useRef(false);
  paletteOpenRef.current = paletteOpen;
  useEffect(
    () =>
      onLibraryContext((snapshot) => {
        libraryContextRef.current = snapshot;
        if (paletteOpenRef.current) setPaletteContext(snapshot);
      }),
    [],
  );

  const openPalette = useCallback((initialQuery = "") => {
    setPaletteQuery(initialQuery);
    setPaletteContext(libraryContextRef.current);
    setPaletteOpen(true);
    requestLibraryContext();
  }, []);

  const runSteamSync = useCallback(() => {
    if (syncRunning.current) {
      setSyncAnnouncement("Ya hay una sincronización de Steam en curso.");
      return;
    }
    if (
      !bootstrap?.steam.account ||
      !bootstrap.steam.apiKeyConfigured ||
      bootstrap.steam.apiKeyVerificationRequired
    ) {
      setSyncAnnouncement(
        "No se puede sincronizar: vincula Steam y comprueba tu Web API Key en Ajustes.",
      );
      return;
    }
    syncRunning.current = true;
    setSyncAnnouncement("Sincronizando manualmente con Steam…");
    void api
      .syncSteamLibrary()
      .then(() => setSyncAnnouncement("Sincronización manual completada."))
      .catch((cause) =>
        setSyncAnnouncement(`Sincronización manual fallida: ${getErrorMessage(cause)}`),
      )
      .finally(() => {
        syncRunning.current = false;
        void invalidateSteamDerivedQueries(queryClient);
      });
  }, [bootstrap, queryClient]);

  const changeDensity = useCallback(
    (density: InterfaceDensity) => {
      if (!bootstrap) return;
      if (bootstrap.preferences.density === density) return;
      void api
        .savePreferences({ ...bootstrap.preferences, density })
        .then(() => {
          setSyncAnnouncement(
            density === "compact" ? "Densidad compacta aplicada." : "Densidad cómoda aplicada.",
          );
          return queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
        })
        .catch((cause) =>
          setSyncAnnouncement(`No se pudo cambiar la densidad: ${getErrorMessage(cause)}`),
        );
    },
    [bootstrap, queryClient],
  );

  const clearArtCache = useCallback(() => {
    setSyncAnnouncement("Vaciando la caché de arte…");
    void api
      .clearArtCache()
      .then(() => setSyncAnnouncement("Caché de arte vaciada."))
      .catch((cause) =>
        setSyncAnnouncement(`No se pudo vaciar la caché de arte: ${getErrorMessage(cause)}`),
      );
  }, []);

  const maintainArtCache = useCallback(() => {
    setSyncAnnouncement("Depurando la caché de arte…");
    void api
      .maintainArtCache()
      .then((report) =>
        setSyncAnnouncement(
          `Caché de arte depurada: ${report.removedFiles.toLocaleString("es-ES")} archivos y ${report.evictedFiles.toLocaleString("es-ES")} desalojos.`,
        ),
      )
      .catch((cause) =>
        setSyncAnnouncement(`No se pudo depurar la caché de arte: ${getErrorMessage(cause)}`),
      );
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat || isEditableShortcutTarget(event.target)) return;
      // Con la paleta abierta manda la paleta: es un diálogo modal.
      if (paletteOpen) return;
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        setSettingsOpen(true);
        return;
      }
      const descriptor = resolveShortcutEvent(event, shortcuts);
      if (!descriptor) return;
      switch (descriptor.action) {
        case "library":
        case "planner":
        case "collections":
        case "tracking":
          event.preventDefault();
          setSection(descriptor.action);
          return;
        case "gotoWishlist":
          event.preventDefault();
          setSection("wishlist");
          return;
        case "couchMode":
          event.preventDefault();
          setSection("couch");
          return;
        case "closePanel":
          event.preventDefault();
          // En el modo sofá no hay panel que cerrar: lo que se cierra es el modo.
          if (section === "couch") setSection("library");
          else if (settingsOpen) setSettingsOpen(false);
          else window.dispatchEvent(new CustomEvent(CLOSE_PANEL_EVENT));
          return;
        case "search":
          event.preventDefault();
          setSection("library");
          window.requestAnimationFrame(() =>
            window.dispatchEvent(new CustomEvent(FOCUS_SEARCH_EVENT)),
          );
          return;
        case "sync":
          event.preventDefault();
          runSteamSync();
          return;
        case "commandPalette":
          event.preventDefault();
          openPalette();
          return;
        default:
          break;
      }

      // A partir de aquí sólo actúan los atajos operativos, y sólo cuando la
      // biblioteca está delante y no hay un diálogo por encima.
      if (settingsOpen || section !== "library") return;
      // `Intro`, `Espacio`, flechas, `Inicio`, `Fin` y `Supr` pertenecen al
      // control con el foco en cualquier otro sitio de la aplicación.
      if (
        requiresLibrarySurface(shortcuts[descriptor.action]) &&
        !isLibrarySurfaceTarget(event.target)
      ) {
        return;
      }
      switch (descriptor.action) {
        case "focusUp":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "up", extend: false });
          return;
        case "focusDown":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "down", extend: false });
          return;
        case "focusLeft":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "left", extend: false });
          return;
        case "focusRight":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "right", extend: false });
          return;
        case "focusFirst":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "first", extend: false });
          return;
        case "focusLast":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "last", extend: false });
          return;
        case "extendUp":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "up", extend: true });
          return;
        case "extendDown":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "down", extend: true });
          return;
        case "extendLeft":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "left", extend: true });
          return;
        case "extendRight":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "moveFocus", direction: "right", extend: true });
          return;
        case "selectAll":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "selectAll" });
          return;
        case "undo":
          // Deshacer no exige juego enfocado: se resuelve antes del bloque que
          // pide foco, porque el último cambio puede haber sido masivo.
          if (hasTextSelection()) return;
          event.preventDefault();
          dispatchLibraryCommand({ kind: "undo" });
          return;
        default:
          break;
      }

      const appId = libraryContextRef.current?.focusedAppId;
      if (appId === undefined) return;
      switch (descriptor.action) {
        case "primaryAction":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "primary", appId });
          return;
        case "openDetail":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "openDetail", appId });
          return;
        case "openStore":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "openStore", appId });
          return;
        case "revealInstallation":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "reveal", appId });
          return;
        case "requestUninstall":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "uninstall", appId });
          return;
        case "togglePinned":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "togglePinned", appId });
          return;
        case "toggleTracking":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "toggleTracking", appId });
          return;
        case "statusForward":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "cycleStatus", appId, direction: 1 });
          return;
        case "statusBackward":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "cycleStatus", appId, direction: -1 });
          return;
        case "statusSlot1":
        case "statusSlot2":
        case "statusSlot3":
        case "statusSlot4":
        case "statusSlot5": {
          // El número es la posición en la lista de estados que la persona ve,
          // no un identificador: si reordena sus estados, Alt+1 sigue llevando
          // al primero de su lista.
          const slot = Number(descriptor.action.slice("statusSlot".length)) - 1;
          const target = libraryContextRef.current?.statuses[slot];
          if (!target) return;
          event.preventDefault();
          dispatchLibraryCommand({ kind: "setStatus", appId, statusId: target.id });
          return;
        }
        case "addToPlanner": {
          event.preventDefault();
          const selected = libraryContextRef.current?.selectedAppIds ?? [];
          const appIds = selected.length > 1 ? selected : [appId];
          dispatchLibraryCommand({ kind: "addToPlanner", appIds });
          return;
        }
        case "priorityUp":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "shiftPriority", appId, direction: 1 });
          return;
        case "priorityDown":
          event.preventDefault();
          dispatchLibraryCommand({ kind: "shiftPriority", appId, direction: -1 });
          return;
        case "addToCollection":
          event.preventDefault();
          openPalette("colección");
          return;
        case "copyTitle":
          // Copiar nunca debe pisar una selección de texto real.
          if (hasTextSelection()) return;
          event.preventDefault();
          dispatchLibraryCommand({ kind: "copyTitle", appId });
          return;
        case "copyAppId":
          if (hasTextSelection()) return;
          event.preventDefault();
          dispatchLibraryCommand({ kind: "copyAppId", appId });
          return;
        default:
          return;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openPalette, paletteOpen, runSteamSync, section, settingsOpen, shortcuts]);

  const steamAccount = bootstrap?.steam.account;
  const steamHealth = steamAccount
    ? steamAccount.lastSyncStatus === "failed"
      ? {
          state: "failed",
          label: "Cuenta vinculada · sincronización fallida",
          compactLabel: "Steam · sync fallida",
          footer: "Steam · sincronización fallida",
        }
      : steamAccount.lastSyncStatus === "success"
        ? {
            state: "success",
            label: "Cuenta vinculada · sincronizada",
            compactLabel: "Steam · al día",
            footer: "Steam · sincronización correcta",
          }
        : {
            state: "never",
            label: "Cuenta vinculada · sin sincronizar",
            compactLabel: "Steam · sin sincronizar",
            footer: "Steam · pendiente de sincronizar",
          }
    : undefined;
  useEffect(() => {
    const minutes = bootstrap?.preferences.periodicSyncMinutes ?? 0;
    if (
      !bootstrap?.steam.account ||
      !bootstrap.steam.apiKeyConfigured ||
      bootstrap.steam.apiKeyVerificationRequired ||
      minutes <= 0
    )
      return;
    const interval = window.setInterval(() => {
      if (syncRunning.current) return;
      syncRunning.current = true;
      void api
        .syncSteamLibrary()
        .then(() => setSyncAnnouncement("Sincronización periódica completada."))
        .catch((cause) => {
          console.error("Sincronización periódica fallida", cause);
          setSyncAnnouncement(`Sincronización periódica fallida: ${getErrorMessage(cause)}`);
        })
        .finally(() => {
          syncRunning.current = false;
          void invalidateSteamDerivedQueries(queryClient);
        });
    }, minutes * 60_000);
    return () => window.clearInterval(interval);
  }, [
    bootstrap?.preferences.periodicSyncMinutes,
    bootstrap?.steam.account,
    bootstrap?.steam.apiKeyConfigured,
    bootstrap?.steam.apiKeyVerificationRequired,
    queryClient,
  ]);
  const screenProps = bootstrap
    ? { bootstrap, loading: bootstrapQuery.isPending }
    : { loading: bootstrapQuery.isPending };
  const content = (() => {
    if (section === "planner") return <PlannerScreen {...screenProps} />;
    if (section === "wishlist") return <WishlistScreen {...screenProps} />;
    if (section === "collections") return <CollectionsScreen {...screenProps} />;
    if (section === "tracking") return <DiscoveryScreen {...screenProps} />;
    return (
      <LibraryScreen
        bootstrap={bootstrap}
        loading={bootstrapQuery.isPending}
        error={bootstrapQuery.error}
        onRetry={() => bootstrapQuery.refetch()}
      />
    );
  })();

  const platform = /Macintosh|Mac OS X/.test(navigator.userAgent) ? "macos" : "other";
  const density = bootstrap?.preferences.density ?? "compact";

  /**
   * El modo sofá se mira a dos metros: ocupa la ventana entera y prescinde de
   * la barra superior y de la de estado, porque a esa distancia ese cromado no
   * se lee y sólo le quita sitio a las carátulas. Los atajos de navegación
   * siguen vivos, así que `Mod+1` devuelve a la biblioteca igual que la B.
   */
  if (section === "couch") {
    return (
      <Suspense fallback={<LoadingState label="Abriendo el modo sofá" />}>
        <CouchScreen onExit={() => setSection("library")} />
      </Suspense>
    );
  }

  return (
    <InterfaceDensityContext.Provider value={density}>
      <div className="app-shell" data-platform={platform} data-density={density}>
        <span className="sr-only" role="status" aria-live="polite">
          {syncAnnouncement}
        </span>
        <header className="topbar" ref={titleBarRef}>
          <div className="brand">
            <span className="brand__name">VINDEXA</span>
            <span className="brand__edition">DESKTOP</span>
          </div>
          <nav className="primary-nav" aria-label="Secciones principales">
            {sections.map(({ id, label, icon: SectionIcon }) => (
              <button
                key={id}
                type="button"
                className="primary-nav__item"
                data-active={section === id}
                aria-current={section === id ? "page" : undefined}
                aria-label={label}
                onClick={() => setSection(id)}
              >
                <SectionIcon aria-hidden="true" size={16} stroke={1.8} />
                <span>{label}</span>
              </button>
            ))}
          </nav>
          <div className="topbar__actions">
            {steamAccount && steamHealth ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    className="account-chip"
                    type="button"
                    data-sync-state={steamHealth.state}
                    aria-label={`${steamHealth.label}. Abrir ajustes de Steam`}
                    onClick={() => setSettingsOpen(true)}
                  >
                    {steamAccount.avatarUrl ? (
                      <img src={steamAccount.avatarUrl} alt="" />
                    ) : (
                      <IconBrandSteam aria-hidden="true" size={15} />
                    )}
                    <span className="account-chip__label">{steamHealth.compactLabel}</span>
                    <span
                      className="presence-dot"
                      data-state={steamHealth.state}
                      aria-hidden="true"
                    />
                  </button>
                </TooltipTrigger>
                <TooltipContent>
                  {steamHealth.state === "failed" && steamAccount.lastSyncErrorMessage
                    ? steamAccount.lastSyncErrorMessage
                    : `${steamHealth.label}.`}{" "}
                  Abrir ajustes
                </TooltipContent>
              </Tooltip>
            ) : (
              <span className="sync-status">
                <i /> Solo local
              </span>
            )}
            <NotificationsPopover />
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Entrar en el modo sofá"
                  onClick={() => setSection("couch")}
                >
                  <IconDeviceGamepad2 />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                Modo sofá <kbd>{shortcutLabel(shortcuts.couchMode)}</kbd>
              </TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Abrir la paleta de comandos"
                  onClick={() => openPalette()}
                >
                  <IconCommand />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                Paleta de comandos <kbd>{shortcutLabel(shortcuts.commandPalette)}</kbd>
              </TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Actualizar datos"
                  onClick={() => queryClient.invalidateQueries()}
                  disabled={bootstrapQuery.isFetching}
                >
                  <IconRefresh className={bootstrapQuery.isFetching ? "is-spinning" : undefined} />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Actualizar datos</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Abrir ajustes"
                  onClick={() => setSettingsOpen(true)}
                >
                  <IconSettings />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                Ajustes <kbd>⌘,</kbd>
              </TooltipContent>
            </Tooltip>
          </div>
        </header>
        <main className="app-content">
          <Suspense fallback={<LoadingState label="Abriendo sección" />}>{content}</Suspense>
        </main>
        <footer className="statusbar">
          <span>
            {bootstrap
              ? [
                  `Biblioteca · ${bootstrap.stats.totalGames.toLocaleString("es-ES")} juegos`,
                  `${bootstrap.stats.installedGames.toLocaleString("es-ES")} instalados`,
                  // El catálogo de Family va aparte porque tenerlo a la vista no
                  // es tenerlo en propiedad. Se nombra aquí igualmente: es la
                  // cifra que falta al comparar con el cliente de Steam, y sin
                  // ella parece que esos juegos no están.
                  ...(bootstrap.stats.familyCatalogGames > 0
                    ? [
                        `${bootstrap.stats.familyCatalogGames.toLocaleString("es-ES")} en Steam Family`,
                      ]
                    : []),
                ].join(" · ")
              : "Preparando biblioteca local…"}
          </span>
          <span className="statusbar__center">
            {metadata
              ? `SQLite · metadatos ${metadata.freshMetadata.toLocaleString("es-ES")}/${metadata.totalGames.toLocaleString("es-ES")}${metadata.retrying ? " · en pausa" : metadata.running ? " · indexando" : ""}${metadata.failed ? ` · ${metadata.failed.toLocaleString("es-ES")} errores` : ""}`
              : "SQLite · datos locales"}
          </span>
          <span>{steamHealth?.footer ?? "Steam no vinculado"}</span>
        </footer>
        {paletteOpen && (
          <Suspense fallback={null}>
            <CommandPalette
              open={paletteOpen}
              onOpenChange={setPaletteOpen}
              bindings={shortcuts}
              section={section}
              density={density}
              context={paletteContext}
              initialQuery={paletteQuery}
              onNavigate={setSection}
              onOpenSettings={() => setSettingsOpen(true)}
              onSync={runSteamSync}
              onSetDensity={changeDensity}
              onClearArtCache={clearArtCache}
              onMaintainArtCache={maintainArtCache}
            />
          </Suspense>
        )}
        {settingsOpen && (
          <Suspense fallback={null}>
            <SettingsDialog
              open={settingsOpen}
              onOpenChange={setSettingsOpen}
              bootstrap={bootstrap}
            />
          </Suspense>
        )}
      </div>
    </InterfaceDensityContext.Provider>
  );
}
