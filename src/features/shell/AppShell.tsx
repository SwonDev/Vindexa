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
  IconRobot,
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
import { ToastProvider } from "@/features/shell/toasts";
import {
  fitWindowToAvailableSpace,
  isEmptyChromeTarget,
  toggleFitWindow,
} from "@/features/shell/window-chrome";
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
// El tipo viaja aparte de la carga diferida: sólo describe la sección, no
// arrastra el diálogo al paquete inicial.
type SettingsSection = import("@/features/settings/SettingsDialog").SettingsSection;
const AgentChatPanel = lazy(() =>
  import("@/features/agent/AgentChatPanel").then((module) => ({
    default: module.AgentChatPanel,
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [agentOpen, setAgentOpen] = useState(false);
  // En qué sección se abre. Quien llega desde un atajo de la biblioteca ya sabe
  // a qué venía; volver a buscarla sería devolverle el trabajo.
  const [settingsSection, setSettingsSection] = useState<SettingsSection | undefined>(undefined);
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

  // Abrir ocupando el espacio disponible, una sola vez: después la ventana es
  // de quien la usa.
  useEffect(() => {
    void fitWindowToAvailableSpace();
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
        onOpenSettings={(target) => {
          setSettingsSection(target);
          setSettingsOpen(true);
        }}
      />
    );
  })();

  /**
   * Lo que la barra de estado tiene derecho a ocupar sitio para contar.
   *
   * El criterio es que no esté ya visible en otro lado: el número de juegos lo
   * dice la barra lateral, el estado de Steam lo dice su ficha de la cabecera y
   * que la base de datos funcione no es noticia. Queda el trabajo que está
   * ocurriendo ahora y lo que ha salido mal.
   */
  const statusNotices = useMemo(() => {
    const notices: { id: string; text: string; kind: "progress" | "warning" }[] = [];
    if (metadata?.running) {
      const pendientes = metadata.totalGames - metadata.freshMetadata;
      notices.push({
        id: "metadata",
        kind: "progress",
        text: `Completando fichas · faltan ${pendientes.toLocaleString("es-ES")} de ${metadata.totalGames.toLocaleString("es-ES")}`,
      });
    } else if (metadata?.retrying) {
      notices.push({
        id: "metadata-retry",
        kind: "warning",
        text: `Steam ha pedido esperar · ${metadata.retrying.toLocaleString("es-ES")} fichas en cola`,
      });
    }
    if (metadata?.failed) {
      notices.push({
        id: "metadata-failed",
        kind: "warning",
        text: `${metadata.failed.toLocaleString("es-ES")} fichas no se han podido leer`,
      });
    }
    // El punto de color de la cabecera dice que algo va mal; aquí cabe el qué.
    if (steamHealth?.state === "failed") {
      notices.push({ id: "steam", kind: "warning", text: steamHealth.footer });
    }
    return notices;
  }, [metadata, steamHealth]);

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
      {/* Los avisos envuelven a toda la carcasa: lo que se lanza desde un menú
          contextual, desde la barra lateral o desde una pantalla se cuenta en
          el mismo sitio y de la misma manera. */}
      <ToastProvider>
        <div className="app-shell" data-platform={platform} data-density={density}>
          <span className="sr-only" role="status" aria-live="polite">
            {syncAnnouncement}
          </span>
          {/* `deep` hace que cualquier zona inerte de la barra arrastre la
            ventana y que el doble clic la maximice. El script de Tauri excluye
            solo botones, enlaces y campos, que es justo lo que aquí interesa
            que siga siendo pulsable. */}
          {/* El doble clic en la parte vacía lo resuelve Vindexa, no el sistema.
              Tauri trae el suyo: en macOS maximiza al soltar y en Windows y
              Linux al pulsar, y lo que hace es el *zoom* del sistema, que lleva
              la ventana al tamaño preferido de la aplicación y no al que cabe en
              la pantalla.

              Los dos gestos a la vez se pisaban —el sistema encogía y Vindexa
              volvía a estirar—, así que aquí se corta el suyo y se hace el
              nuestro: el manejador de Tauri escucha en `document`, y detener la
              propagación en la barra impide que llegue. Sólo se corta el doble
              clic (`detail === 2`); el arrastre, que es un clic simple, sigue
              siendo suyo y sigue funcionando igual. */}
          <header
            className="topbar"
            data-tauri-drag-region="deep"
            onMouseDown={(event) => {
              // Windows y Linux maximizan aquí. Se les quita el turno.
              if (event.detail === 2 && isEmptyChromeTarget(event.target)) {
                event.stopPropagation();
              }
            }}
            onMouseUp={(event) => {
              // macOS maximiza aquí, para poder cancelar el gesto moviendo el
              // ratón. Se le quita el turno y se hace lo que se pidió.
              if (event.detail !== 2 || !isEmptyChromeTarget(event.target)) return;
              event.stopPropagation();
              void toggleFitWindow();
            }}
          >
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
                    <IconRefresh
                      className={bootstrapQuery.isFetching ? "is-spinning" : undefined}
                    />
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
          {/* La barra de estado sólo existe cuando tiene algo que decir.
            Repetía tres cosas que ya están en pantalla —el recuento de la
            biblioteca, el estado de Steam y el detalle de SQLite— y a cambio se
            comía una fila de alto en todas las pantallas. Ahora enseña trabajo
            en curso y avisos, que es lo único que no se ve en ningún otro
            sitio, y cuando no hay ni una cosa ni otra desaparece. */}
          <footer className="statusbar">
            <span role="status" className="statusbar__notices">
              {statusNotices.map((notice) => (
                <span key={notice.id} data-kind={notice.kind}>
                  {notice.text}
                </span>
              ))}
            </span>
            {/* El agente vive aquí porque desde aquí se le habla mientras se
                mira la biblioteca, no en lugar de mirarla. */}
            <button
              type="button"
              className="statusbar__agent"
              data-open={agentOpen}
              aria-expanded={agentOpen}
              onClick={() => setAgentOpen((open) => !open)}
            >
              <IconRobot aria-hidden="true" /> Agente
            </button>
          </footer>
          {agentOpen && (
            <Suspense fallback={null}>
              <AgentChatPanel onClose={() => setAgentOpen(false)} />
            </Suspense>
          )}
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
                onOpenChange={(open) => {
                  setSettingsOpen(open);
                  if (!open) setSettingsSection(undefined);
                }}
                bootstrap={bootstrap}
                initialSection={settingsSection}
              />
            </Suspense>
          )}
        </div>
      </ToastProvider>
    </InterfaceDensityContext.Provider>
  );
}
