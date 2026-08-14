import {
  IconBooks,
  IconBrandSteam,
  IconChecklist,
  IconFolders,
  IconLayoutKanban,
  IconRefresh,
  IconSettings,
} from "@tabler/icons-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { LoadingState } from "@/components/common/LoadingState";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { LibraryScreen } from "@/features/library/LibraryScreen";
import { InterfaceDensityContext } from "@/features/shell/interface-density";
import {
  DEFAULT_SHORTCUTS,
  isEditableShortcutTarget,
  matchesShortcut,
} from "@/features/shell/shortcuts";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppSection } from "@/lib/types";
import vindexaIcon from "../../../assets/brand/vindexa-mark-256.webp";

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

const sections = [
  { id: "library", label: "Biblioteca", icon: IconBooks },
  { id: "planner", label: "Planificador", icon: IconLayoutKanban },
  { id: "collections", label: "Colecciones", icon: IconFolders },
  { id: "tracking", label: "Seguimiento", icon: IconChecklist },
] satisfies { id: AppSection; label: string; icon: typeof IconBooks }[];

export function AppShell() {
  const [section, setSection] = useState<AppSection>("library");
  const [settingsOpen, setSettingsOpen] = useState(false);
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
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat || isEditableShortcutTarget(event.target)) return;
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        setSettingsOpen(true);
        return;
      }
      const shortcuts = bootstrap?.preferences.shortcuts ?? DEFAULT_SHORTCUTS;
      if (matchesShortcut(event, shortcuts.closePanel)) {
        event.preventDefault();
        if (settingsOpen) setSettingsOpen(false);
        else window.dispatchEvent(new CustomEvent("vindexa:close-panel"));
        return;
      }
      const navigation = (["library", "planner", "collections", "tracking"] as const).find(
        (candidate) => matchesShortcut(event, shortcuts[candidate]),
      );
      if (navigation) {
        event.preventDefault();
        setSection(navigation);
        return;
      }
      if (matchesShortcut(event, shortcuts.search)) {
        event.preventDefault();
        setSection("library");
        window.requestAnimationFrame(() =>
          window.dispatchEvent(new CustomEvent("vindexa:focus-search")),
        );
        return;
      }
      if (matchesShortcut(event, shortcuts.sync)) {
        event.preventDefault();
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
            void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
            void queryClient.invalidateQueries({ queryKey: ["games"] });
          });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [bootstrap, queryClient, settingsOpen]);

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
          void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
          void queryClient.invalidateQueries({ queryKey: ["games"] });
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
  return (
    <InterfaceDensityContext.Provider value={density}>
      <div className="app-shell" data-platform={platform} data-density={density}>
        <span className="sr-only" role="status" aria-live="polite">
          {syncAnnouncement}
        </span>
        <header className="topbar">
          <div className="brand">
            <img src={vindexaIcon} alt="Vindexa" className="brand__mark" />
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
              ? `${bootstrap.stats.totalGames.toLocaleString("es-ES")} juegos · ${bootstrap.stats.installedGames.toLocaleString("es-ES")} instalados`
              : "Preparando biblioteca local…"}
          </span>
          <span className="statusbar__center">
            {metadata
              ? `SQLite · metadatos ${metadata.freshMetadata.toLocaleString("es-ES")}/${metadata.totalGames.toLocaleString("es-ES")}${metadata.retrying ? " · en pausa" : metadata.running ? " · indexando" : ""}${metadata.failed ? ` · ${metadata.failed.toLocaleString("es-ES")} errores` : ""}`
              : "SQLite · datos locales"}
          </span>
          <span>{steamHealth?.footer ?? "Steam no vinculado"}</span>
        </footer>
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
