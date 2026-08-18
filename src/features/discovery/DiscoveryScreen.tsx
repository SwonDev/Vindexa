import {
  IconBell,
  IconBellPlus,
  IconBolt,
  IconCalendarEvent,
  IconCheck,
  IconClock,
  IconDeviceGamepad2,
  IconEye,
  IconHistory,
  IconLoader2,
  IconMoodSmile,
  IconRefresh,
  IconRestore,
  IconSparkles,
  IconTimelineEvent,
  type Icon as IconType,
  IconWand,
  IconX,
} from "@tabler/icons-react";
import {
  type UseMutationResult,
  type UseQueryResult,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { type KeyboardEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { EmptyState } from "@/components/common/EmptyState";
import { Eyebrow } from "@/components/common/Eyebrow";
import { LoadingState } from "@/components/common/LoadingState";
import { PageHeader } from "@/components/common/PageHeader";
import { ProgressMeter } from "@/components/common/ProgressMeter";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { UpcomingReleasesBlock } from "@/features/discovery/UpcomingReleasesBlock";
import { NotificationRulesPanel } from "@/features/notifications/NotificationRulesPanel";
import { formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  AppBootstrap,
  DiscoveryEvent,
  DiscoverySnapshot,
  DismissedRecommendation,
  GameReminder,
  GameSummary,
  NewsRefreshReport,
  OfficialPublication,
  Recommendation,
  RelatedRelease,
} from "@/lib/types";
import "./discovery.css";

type RadarView = "tracking" | "reminders" | "forgotten" | "almost";

interface RadarDefinition {
  id: RadarView;
  label: string;
  icon: IconType;
  description: string;
  emptyTitle: string;
  emptyDescription: string;
}

const radarViews: RadarDefinition[] = [
  {
    id: "tracking",
    label: "Seguimiento",
    icon: IconEye,
    description: "Títulos que sigues para detectar cambios verificables en Steam.",
    emptyTitle: "No sigues ningún juego",
    emptyDescription: "Activa “Seguir actualizaciones” desde la ficha de un título.",
  },
  {
    id: "reminders",
    label: "Recordatorios",
    icon: IconBell,
    description: "Avisos locales con fecha; se posponen o se completan desde aquí.",
    emptyTitle: "No hay recordatorios pendientes",
    emptyDescription: "Añade uno desde Juegos olvidados o Casi terminados; se guardará localmente.",
  },
  {
    id: "forgotten",
    label: "Olvidados",
    icon: IconHistory,
    description: "Sin jugar en 180 días o inactivos durante un año completo.",
    emptyTitle: "No hay juegos olvidados",
    emptyDescription: "Aquí aparecen los no jugados en 180 días y los inactivos durante un año.",
  },
  {
    id: "almost",
    label: "Casi terminados",
    icon: IconCheck,
    description: "Progreso propio registrado entre el 75 % y el 99 %.",
    emptyTitle: "No hay juegos casi terminados",
    emptyDescription: "Esta vista usa únicamente progreso propio entre 75 % y 99 %.",
  },
];

const skeletonKeys = ["a", "b", "c", "d", "e", "f"] as const;

export function DiscoveryScreen({
  bootstrap,
  loading,
}: {
  bootstrap?: AppBootstrap;
  loading: boolean;
}) {
  const queryClient = useQueryClient();
  const [duration, setDuration] = useState("60");
  const [mood, setMood] = useState("any");
  const [radarView, setRadarView] = useState<RadarView>("tracking");
  const [announcement, setAnnouncement] = useState("");
  const discovery = useQuery({
    queryKey: ["discovery"],
    queryFn: api.discoverySnapshot,
  });
  const tracked = useQuery({
    queryKey: ["games", "tracking"],
    queryFn: () => api.listGames({ tracking: true, sort: "lastPlayed", limit: 200 }),
  });
  const newsRefresh = useQuery({
    queryKey: ["discovery-news-refresh"],
    queryFn: async () => {
      const report = await api.refreshDiscoveryNews();
      await queryClient.invalidateQueries({ queryKey: ["discovery"] });
      return report;
    },
    enabled: (bootstrap?.stats.trackedGames ?? 0) > 0,
    staleTime: 10 * 60 * 1000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const recommendation = useMutation({
    mutationFn: () => api.recommendGame(Number(duration), mood === "any" ? undefined : mood),
  });
  /**
   * La pantalla abre con la respuesta, no con la explicación de qué hará.
   *
   * Quien entra aquí ya sabe qué es: viene a que le digan a qué jugar. Se pide
   * una recomendación en cuanto hay biblioteca, y solo una vez por sesión: si
   * la persona la descarta, no se le vuelve a imponer otra sin pedirla.
   */
  const autoRecommended = useRef(false);
  const requestRecommendation = recommendation.mutate;
  const totalGames = bootstrap?.stats.totalGames ?? 0;
  useEffect(() => {
    if (autoRecommended.current || totalGames === 0) return;
    // El disparo se aplaza un tic para no morir en el doble montaje que hace
    // React en modo estricto: una petición lanzada durante el primer montaje se
    // descarta al desmontar y dejaría la tarjeta cargando para siempre.
    const timer = window.setTimeout(() => {
      autoRecommended.current = true;
      requestRecommendation();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [totalGames, requestRecommendation]);
  const refreshDiscovery = () => queryClient.invalidateQueries({ queryKey: ["discovery"] });
  const reminder = useMutation({
    mutationFn: (game: GameSummary) =>
      api.saveReminder({
        appId: game.appId,
        dueAt: addDays(new Date(), 7).toISOString(),
        note: `Retomar ${game.title}`,
      }),
    onSuccess: (saved) => {
      setAnnouncement(`Recordatorio de ${saved.title} creado para dentro de siete días.`);
      void refreshDiscovery();
    },
    onError: (cause) =>
      setAnnouncement(`No se pudo crear el recordatorio: ${getErrorMessage(cause)}`),
  });
  const completeReminder = useMutation({
    mutationFn: (id: string) => api.completeReminder(id),
    onSuccess: () => {
      setAnnouncement("Recordatorio completado.");
      void refreshDiscovery();
    },
    onError: (cause) => setAnnouncement(`No se pudo completar: ${getErrorMessage(cause)}`),
  });
  const snoozeReminder = useMutation({
    mutationFn: (item: GameReminder) =>
      api.snoozeReminder(item.id, addDays(new Date(item.dueAt), 7).toISOString()),
    onSuccess: (saved) => {
      setAnnouncement(`Recordatorio de ${saved.title} pospuesto siete días.`);
      void refreshDiscovery();
    },
    onError: (cause) => setAnnouncement(`No se pudo posponer: ${getErrorMessage(cause)}`),
  });
  const dismiss = useMutation({
    mutationFn: (id: string) => api.dismissRecommendation(id),
    onSuccess: () => {
      const title = recommendation.data?.game.title ?? "el juego";
      setAnnouncement(`${title} se ha descartado de futuras recomendaciones.`);
      recommendation.reset();
      void refreshDiscovery();
    },
    onError: (cause) => setAnnouncement(`No se pudo descartar: ${getErrorMessage(cause)}`),
  });
  const restore = useMutation({
    mutationFn: (id: string) => api.restoreRecommendation(id),
    onSuccess: () => {
      setAnnouncement("El juego volverá a considerarse en próximas recomendaciones.");
      void refreshDiscovery();
    },
    onError: (cause) => setAnnouncement(`No se pudo restaurar: ${getErrorMessage(cause)}`),
  });

  const currentItems = useMemo(() => {
    if (radarView === "tracking") return tracked.data?.items ?? [];
    if (radarView === "forgotten") return discovery.data?.forgotten ?? [];
    if (radarView === "almost") return discovery.data?.almostFinished ?? [];
    return [];
  }, [discovery.data, radarView, tracked.data]);

  const trackedTotal = tracked.data?.total ?? 0;
  const activeView = radarViews.find((item) => item.id === radarView) ?? radarViews[0];
  const radarBusy = discovery.isPending || tracked.isPending;
  const radarFailed = discovery.isError || tracked.isError;
  const radarCountLabel = radarBusy
    ? "Contrastando datos locales"
    : radarFailed
      ? "Datos no disponibles"
      : `${radarCount(radarView, discovery.data, trackedTotal)} elementos en esta vista`;

  if (loading && !bootstrap) return <LoadingState label="Cargando descubrimiento" />;
  if (!activeView) return null;
  return (
    <section className="discovery-screen" aria-labelledby="discovery-title">
      <span className="sr-only" role="status" aria-live="polite">
        {announcement}
      </span>

      {/* Sin tira de métricas: cuatro de sus cinco cifras son exactamente las
          del «Radar personal» que está treinta píxeles más abajo, y la quinta
          —las observaciones de metadatos— es contabilidad interna que ya se
          explica en su propio bloque de señales. La decisión de esta pantalla
          la toma «Jugar ahora», no un marcador. */}
      <PageHeader
        eyebrow="SEGUIMIENTO Y DESCUBRIMIENTO"
        title="Qué jugar ahora"
        titleId="discovery-title"
      />

      <div className="discovery-body">
        <aside className="discovery-rail" aria-label="Controles de seguimiento">
          <section className="rail-block" aria-labelledby="radar-nav-title">
            <h2 className="rail-block__title" id="radar-nav-title">
              Radar personal
            </h2>
            <RadarNav
              view={radarView}
              counts={{
                tracking: trackedTotal,
                reminders: discovery.data?.reminders.length ?? 0,
                forgotten: discovery.data?.forgotten.length ?? 0,
                almost: discovery.data?.almostFinished.length ?? 0,
              }}
              busy={radarBusy}
              onSelect={setRadarView}
            />
          </section>

          <section className="rail-block" aria-labelledby="assist-title">
            <h2 className="rail-block__title" id="assist-title">
              Elección asistida
            </h2>
            <div className="decision-field">
              <span id="decision-duration-label">
                <IconClock aria-hidden="true" /> Tiempo disponible
              </span>
              <Select value={duration} onValueChange={setDuration}>
                <SelectTrigger aria-label="Tiempo disponible">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="30">30 minutos</SelectItem>
                  <SelectItem value="60">1 hora</SelectItem>
                  <SelectItem value="120">2 horas</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="decision-field">
              <span id="decision-mood-label">
                <IconMoodSmile aria-hidden="true" /> Tipo de experiencia
              </span>
              <Select value={mood} onValueChange={setMood}>
                <SelectTrigger aria-label="Tipo de experiencia">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="any">Cualquiera</SelectItem>
                  <SelectItem value="relaxed">Relajada</SelectItem>
                  <SelectItem value="focused">Concentrada</SelectItem>
                  <SelectItem value="competitive">Competitiva</SelectItem>
                  <SelectItem value="story">Narrativa</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <Button
              className="rail-block__action"
              onClick={() => recommendation.mutate()}
              disabled={recommendation.isPending || (bootstrap?.stats.totalGames ?? 0) === 0}
            >
              {recommendation.isPending ? <IconLoader2 className="is-spinning" /> : <IconWand />}
              Elige por mí
            </Button>
            <p className="rail-block__hint">
              La explicación muestra qué datos reales influyeron. Si falta un dato, Vindexa lo dirá.
            </p>
          </section>
        </aside>

        <div className="discovery-main">
          <section className="decision-panel" aria-labelledby="decision-title">
            <div className="panel-heading">
              <div>
                <Eyebrow>DECISIÓN</Eyebrow>
                <h2 id="decision-title">Recomendación explicable</h2>
              </div>
              <span>
                {durationLabel(duration)} · {moodLabel(mood)}
              </span>
            </div>
            <RecommendationResult
              recommendation={recommendation}
              duration={duration}
              mood={mood}
              dismissing={dismiss.isPending}
              onDismiss={(id) => dismiss.mutate(id)}
              onActionError={(message) => setAnnouncement(message)}
            />
          </section>

          <section className="radar-panel" aria-labelledby="radar-title">
            <div className="panel-heading">
              <div>
                <Eyebrow>RADAR PERSONAL</Eyebrow>
                <h2 id="radar-title">{activeView.label}</h2>
              </div>
              <span className="radar-panel__status" aria-live="polite">
                {radarCountLabel}
              </span>
            </div>
            <p className="radar-panel__description">{activeView.description}</p>
            <div
              id="radar-panel"
              className="radar-scroll"
              role="tabpanel"
              // biome-ignore lint/a11y/noNoninteractiveTabindex: zona desplazable; sin foco propio el teclado no puede recorrerla (WCAG 2.1.1)
              tabIndex={0}
              aria-labelledby={`radar-tab-${radarView}`}
              aria-busy={radarBusy}
            >
              {radarBusy ? (
                <RadarSkeleton label="Cargando radar personal" />
              ) : radarFailed ? (
                <RecoverableError
                  message={getErrorMessage(discovery.error ?? tracked.error)}
                  onRetry={() => {
                    void discovery.refetch();
                    void tracked.refetch();
                  }}
                />
              ) : radarView === "reminders" ? (
                <ReminderList
                  items={discovery.data?.reminders ?? []}
                  busy={completeReminder.isPending || snoozeReminder.isPending}
                  onComplete={(id) => completeReminder.mutate(id)}
                  onSnooze={(item) => snoozeReminder.mutate(item)}
                />
              ) : currentItems.length ? (
                <GameRadarList
                  items={currentItems}
                  view={radarView}
                  reminding={reminder.isPending}
                  onRemind={(game) => reminder.mutate(game)}
                />
              ) : (
                <EmptyState
                  compact
                  icon={activeView.icon}
                  title={activeView.emptyTitle}
                  description={activeView.emptyDescription}
                />
              )}
            </div>
          </section>
        </div>

        <aside
          className="discovery-signals"
          aria-label="Señales verificables, próximos lanzamientos y avisos programados"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: zona desplazable; sin foco propio el teclado no puede recorrerla (WCAG 2.1.1)
          tabIndex={0}
        >
          {/* Primero lo que llega y lo que has pedido que te avisen: la columna
              abre con la respuesta, no con el historial. Ambos bloques van
              juntos a propósito —una fecha que te interesa se convierte en un
              aviso sin salir de aquí. */}
          <UpcomingReleasesBlock />
          <NotificationRulesPanel />
          <PublicationsBlock
            items={discovery.data?.officialPublications ?? []}
            trackedGames={discovery.data?.capabilities.trackedNewsGames ?? 0}
            refresh={newsRefresh}
            loading={discovery.isPending}
          />
          <RelatedReleasesBlock
            items={discovery.data?.relatedReleases ?? []}
            loading={discovery.isPending}
          />
          <UpcomingBlock items={discovery.data?.upcoming ?? []} loading={discovery.isPending} />
          <EarlyAccessBlock
            available={discovery.data?.capabilities.earlyAccessHistoryAvailable ?? false}
            observations={discovery.data?.capabilities.metadataObservations ?? 0}
            events={discovery.data?.events ?? []}
            loading={discovery.isPending}
          />
          <DismissedBlock
            items={discovery.data?.dismissedRecommendations ?? []}
            loading={discovery.isPending}
            restoring={restore.isPending}
            onRestore={(id) => restore.mutate(id)}
          />
          {discovery.isError && (
            <RecoverableError
              message={getErrorMessage(discovery.error)}
              onRetry={() => void discovery.refetch()}
            />
          )}
        </aside>
      </div>
    </section>
  );
}

function RadarNav({
  view,
  counts,
  busy,
  onSelect,
}: {
  view: RadarView;
  counts: Record<RadarView, number>;
  busy: boolean;
  onSelect: (next: RadarView) => void;
}) {
  const tabs = useRef(new Map<RadarView, HTMLButtonElement>());
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = radarViews.findIndex((item) => item.id === view);
    let nextIndex = index;
    if (event.key === "ArrowDown" || event.key === "ArrowRight") {
      nextIndex = (index + 1) % radarViews.length;
    } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
      nextIndex = (index - 1 + radarViews.length) % radarViews.length;
    } else if (event.key === "Home") {
      nextIndex = 0;
    } else if (event.key === "End") {
      nextIndex = radarViews.length - 1;
    } else {
      return;
    }
    const next = radarViews[nextIndex];
    if (!next) return;
    event.preventDefault();
    onSelect(next.id);
    tabs.current.get(next.id)?.focus();
  };
  return (
    <div
      className="radar-nav"
      role="tablist"
      aria-orientation="vertical"
      aria-labelledby="radar-nav-title"
      onKeyDown={move}
    >
      {radarViews.map(({ id, label, icon: ViewIcon }) => (
        <button
          key={id}
          id={`radar-tab-${id}`}
          ref={(node) => {
            if (node) tabs.current.set(id, node);
            else tabs.current.delete(id);
          }}
          type="button"
          role="tab"
          className="radar-nav__item"
          aria-selected={view === id}
          aria-controls="radar-panel"
          tabIndex={view === id ? 0 : -1}
          onClick={() => onSelect(id)}
        >
          <ViewIcon aria-hidden="true" />
          <span>{label}</span>
          <strong className="radar-nav__count" data-busy={busy}>
            {busy ? "··" : counts[id]}
          </strong>
        </button>
      ))}
    </div>
  );
}

function RecommendationResult({
  recommendation,
  duration,
  mood,
  dismissing,
  onDismiss,
  onActionError,
}: {
  recommendation: UseMutationResult<Recommendation, Error, void>;
  duration: string;
  mood: string;
  dismissing: boolean;
  onDismiss: (id: string) => void;
  onActionError: (message: string) => void;
}) {
  if (recommendation.isIdle) {
    return (
      <div className="decision-result decision-result--empty">
        <div className="decision-result__empty-copy">
          <IconWand aria-hidden="true" />
          <div>
            <strong>Buscando qué te viene bien ahora</strong>
            <span>
              Con tu tiempo disponible, tu progreso real y los géneros que de verdad juegas.
            </span>
          </div>
        </div>
        <dl className="decision-context" aria-label="Criterios de la próxima recomendación">
          <div>
            <dt>Tiempo</dt>
            <dd>{durationLabel(duration)}</dd>
          </div>
          <div>
            <dt>Experiencia</dt>
            <dd>{moodLabel(mood)}</dd>
          </div>
          <div>
            <dt>Señales</dt>
            <dd>Progreso, seguimiento y géneros reales</dd>
          </div>
        </dl>
        <div className="decision-result__source">
          <IconCheck aria-hidden="true" />
          <span>Si no existe una duración comparable, se indicará sin estimarla.</span>
        </div>
      </div>
    );
  }
  if (recommendation.isPending) {
    return (
      <div className="decision-result decision-result--busy" aria-busy="true">
        <span className="sr-only" role="status">
          Buscando una elección compatible
        </span>
        <span className="skeleton-block decision-result__skeleton-art" aria-hidden="true" />
        <div className="decision-result__skeleton-copy" aria-hidden="true">
          <span className="skeleton-line skeleton-line--eyebrow" />
          <span className="skeleton-line skeleton-line--title" />
          <span className="skeleton-line skeleton-line--meta" />
          <span className="skeleton-line skeleton-line--actions" />
        </div>
      </div>
    );
  }
  if (recommendation.isError) {
    return (
      <RecoverableError
        message={getErrorMessage(recommendation.error)}
        onRetry={() => recommendation.mutate()}
      />
    );
  }
  const result = recommendation.data;
  // Una respuesta sin juego no puede derribar la pantalla entera: se trata como
  // «no hay nada que recomendar ahora» y el resto del radar sigue en pie.
  if (!result?.game) {
    return (
      <div className="decision-result decision-result--empty">
        <div className="decision-result__empty-copy">
          <IconMoodSmile aria-hidden="true" />
          <div>
            <strong>Nada que recomendar ahora mismo</strong>
            <span>Ajusta el tiempo disponible o el tipo de experiencia y vuelve a pedirlo.</span>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="decision-result">
      <Artwork
        appId={result.game.appId}
        src={result.game.headerUrl ?? result.game.coverUrl}
        title={result.game.title}
        kind="header"
        priority
      />
      <div className="decision-result__scrim" />
      <div className="decision-result__content">
        <Eyebrow>RECOMENDACIÓN DE TU BIBLIOTECA</Eyebrow>
        <h3>{result.game.title}</h3>
        <span className="decision-result__stats">
          {formatPlaytime(result.game.playtimeMinutes)} jugados · {result.game.progress}% completado
        </span>
        <div className="decision-reasons">
          {/* Las razones explican; no se pulsan. El relleno de acción está
              reservado a «Jugar ahora», que es lo único accionable aquí. */}
          {result.reasons.map((reason) => (
            <Badge key={reason} variant="outline">
              {reason}
            </Badge>
          ))}
        </div>
        <div className="decision-actions">
          <Button
            size="sm"
            variant={result.game.installed ? "default" : "secondary"}
            onClick={() => {
              const action = result.game.installed
                ? api.launchGame(result.game.appId)
                : api.openStore(result.game.appId);
              void action.catch((cause) =>
                onActionError(`No se pudo abrir el juego: ${getErrorMessage(cause)}`),
              );
            }}
          >
            <IconDeviceGamepad2 /> {result.game.installed ? "Jugar ahora" : "Tienda integrada"}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={dismissing}
            onClick={() => onDismiss(result.historyId)}
          >
            <IconX /> No me apetece
          </Button>
        </div>
      </div>
    </div>
  );
}

function GameRadarList({
  items,
  view,
  reminding,
  onRemind,
}: {
  items: GameSummary[];
  view: Exclude<RadarView, "reminders">;
  reminding: boolean;
  onRemind: (game: GameSummary) => void;
}) {
  return (
    <ul className="radar-list">
      {items.map((game) => (
        <li key={game.appId}>
          <Artwork
            appId={game.appId}
            src={game.iconUrl ?? game.coverUrl}
            title={game.title}
            kind="icon"
          />
          <div className="radar-list__copy">
            <strong>{game.title}</strong>
            <span>{radarMetadata(game, view)}</span>
          </div>
          <ProgressMeter
            className="radar-list__progress"
            value={game.progress}
            label={`Progreso de ${game.title}: ${game.progress}%`}
          />
          {view === "tracking" ? (
            /* Una etiqueta pasiva no puede llevar el relleno del botón
               principal: compite con la única acción real de la pantalla. */
            <Badge variant="outline" data-installed={game.installed}>
              {game.installed ? "Instalado" : "No instalado"}
            </Badge>
          ) : (
            <Button size="sm" variant="outline" disabled={reminding} onClick={() => onRemind(game)}>
              <IconBellPlus /> Recordarme
            </Button>
          )}
        </li>
      ))}
    </ul>
  );
}

function ReminderList({
  items,
  busy,
  onComplete,
  onSnooze,
}: {
  items: GameReminder[];
  busy: boolean;
  onComplete: (id: string) => void;
  onSnooze: (item: GameReminder) => void;
}) {
  if (!items.length) {
    return (
      <EmptyState
        compact
        icon={IconBell}
        title="No hay recordatorios pendientes"
        description="Añade uno desde Juegos olvidados o Casi terminados; se guardará localmente."
      />
    );
  }
  return (
    <ul className="radar-list reminder-list">
      {items.map((item) => (
        <li key={item.id}>
          <Artwork appId={item.appId} src={item.iconUrl} title={item.title} kind="icon" />
          <div className="radar-list__copy">
            <strong>{item.title}</strong>
            <span>
              {item.note || "Sin nota"} · {formatReminderDate(item.dueAt)}
            </span>
          </div>
          <div className="radar-list__actions">
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => onSnooze(item)}>
              <IconCalendarEvent /> +7 días
            </Button>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => onComplete(item.id)}>
              <IconCheck /> Hecho
            </Button>
          </div>
        </li>
      ))}
    </ul>
  );
}

function RadarSkeleton({ label }: { label: string }) {
  return (
    <div className="radar-skeleton">
      <span className="sr-only" role="status">
        {label}
      </span>
      {skeletonKeys.map((token) => (
        <div className="radar-skeleton__row" key={`radar-skeleton-${token}`} aria-hidden="true">
          <span className="skeleton-block" />
          <div>
            <span className="skeleton-line skeleton-line--title" />
            <span className="skeleton-line skeleton-line--meta" />
          </div>
          <span className="skeleton-line skeleton-line--progress" />
        </div>
      ))}
    </div>
  );
}

function SignalSkeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="signal-skeleton" aria-hidden="true">
      {skeletonKeys.slice(0, lines).map((token) => (
        <span className="skeleton-line" key={`signal-skeleton-${token}`} />
      ))}
    </div>
  );
}

function SignalBlock({
  icon: BlockIcon,
  title,
  status,
  headingId,
  state,
  action,
  note,
  children,
}: {
  icon: IconType;
  title: string;
  status: string;
  headingId: string;
  state: "ready" | "unavailable";
  action?: ReactNode;
  /** Procedencia del dato: sigue siendo verdad, deja de ocupar el cuerpo. */
  note?: string;
  children: ReactNode;
}) {
  return (
    <section
      className="signal-block"
      data-state={state}
      aria-labelledby={headingId}
      title={note || undefined}
    >
      <div className="signal-block__heading">
        <BlockIcon aria-hidden="true" />
        <div>
          <h2 id={headingId}>{title}</h2>
          <span aria-live="polite">{status}</span>
        </div>
        {action}
      </div>
      {/* La salvedad viaja en el `title` del bloque y en texto para lectores de
          pantalla: el dato no pierde su procedencia, pero el producto deja de
          defenderse por escrito delante de cada lista. */}
      {note ? <p className="sr-only">{note}</p> : null}
      <div className="signal-block__body">{children}</div>
    </section>
  );
}

function PublicationsBlock({
  items,
  trackedGames,
  refresh,
  loading,
}: {
  items: OfficialPublication[];
  trackedGames: number;
  refresh: UseQueryResult<NewsRefreshReport, Error>;
  loading: boolean;
}) {
  const unavailable = trackedGames === 0;
  const status = loading
    ? "Contrastando cambios"
    : refresh.isFetching
      ? "Consultando Steam"
      : refresh.isError
        ? items.length
          ? "Caché local · actualización pendiente"
          : "Consulta no disponible"
        : refresh.data?.failedGames
          ? `${refresh.data.failedGames} juegos pendientes · caché local`
          : items.length
            ? `${items.length} publicaciones verificadas`
            : "Sin publicaciones recientes";
  return (
    <SignalBlock
      icon={IconBolt}
      title="Publicaciones oficiales recientes"
      headingId="signal-publications-title"
      status={status}
      state={unavailable ? "unavailable" : "ready"}
      note="Feed público oficial de Steam. Este método no expone una señal de importancia: Vindexa muestra cada publicación con su fuente y fecha, sin clasificarla."
      action={
        <Button
          className="signal-block__refresh"
          size="sm"
          variant="ghost"
          disabled={unavailable || refresh.isFetching}
          onClick={() => void refresh.refetch()}
        >
          {refresh.isFetching ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}
          Actualizar
        </Button>
      }
    >
      {loading ? (
        <SignalSkeleton />
      ) : unavailable ? (
        <p className="signal-empty">
          Sigue al menos un juego para consultar su feed sin usar tu Web API Key.
        </p>
      ) : refresh.isError && !items.length ? (
        <div className="signal-error" role="alert">
          <span>{getErrorMessage(refresh.error)}</span>
          <button type="button" onClick={() => void refresh.refetch()}>
            Reintentar
          </button>
        </div>
      ) : refresh.isFetching && !items.length ? (
        <p className="signal-empty" role="status">
          Contrastando el feed oficial…
        </p>
      ) : items.length ? (
        <ul className="signal-list" aria-label="Publicaciones verificadas de Steam">
          {items.slice(0, 4).map((item) => (
            <li key={`${item.appId}-${item.gid}`}>
              <div>
                <strong>{item.title}</strong>
                <span>{item.gameTitle}</span>
              </div>
              <time dateTime={item.publishedAt}>{formatDate(item.publishedAt)}</time>
              <small>{item.feedLabel}</small>
              {!!item.contentPreview && <p>{item.contentPreview}</p>}
            </li>
          ))}
        </ul>
      ) : (
        <p className="signal-empty">
          Steam no devolvió publicaciones recientes para los juegos seguidos.
        </p>
      )}
    </SignalBlock>
  );
}

function RelatedReleasesBlock({ items, loading }: { items: RelatedRelease[]; loading: boolean }) {
  return (
    <SignalBlock
      icon={IconSparkles}
      title="Lanzamientos relacionados"
      headingId="signal-related-title"
      status={
        loading
          ? "Contrastando cambios"
          : items.length
            ? `${items.length} relaciones verificadas`
            : "Sin coincidencias"
      }
      state={items.length ? "ready" : "unavailable"}
      note="Solo títulos importados con fecha real y la misma empresa normalizada. No se infieren relaciones mediante IA."
    >
      {loading ? (
        <SignalSkeleton lines={2} />
      ) : items.length ? (
        <ul className="signal-list" aria-label="Lanzamientos relacionados verificables">
          {items.slice(0, 4).map((item) => (
            <li key={`${item.appId}-${item.relatedToAppId}-${item.criterion}`}>
              <div>
                <strong>{item.title}</strong>
                <span>
                  {item.criterion === "developer" ? "Mismo desarrollador" : "Mismo editor"} ·{" "}
                  {item.criterionValue}
                </span>
              </div>
              <time dateTime={item.releaseDate}>{formatDate(item.releaseDate)}</time>
              <small>Relacionado con {item.relatedToTitle}</small>
            </li>
          ))}
        </ul>
      ) : (
        <p className="signal-empty">
          Aún no hay un lanzamiento futuro que coincida exactamente con un desarrollador o editor de
          un juego que hayas usado.
        </p>
      )}
    </SignalBlock>
  );
}

function UpcomingBlock({ items, loading }: { items: GameSummary[]; loading: boolean }) {
  return (
    <SignalBlock
      icon={IconCalendarEvent}
      title="Próximos de tu biblioteca"
      headingId="signal-upcoming-title"
      status={
        loading
          ? "Contrastando cambios"
          : items.length
            ? `${items.length} fechas reales`
            : "Sin fechas futuras"
      }
      state={items.length ? "ready" : "unavailable"}
    >
      {loading ? (
        <SignalSkeleton lines={2} />
      ) : items.length ? (
        <ul className="signal-list signal-list--tight" aria-label="Próximos lanzamientos fechados">
          {items.slice(0, 4).map((game) => (
            <li key={game.appId}>
              <div>
                <strong>{game.title}</strong>
              </div>
              <time dateTime={game.releaseDate}>{formatDate(game.releaseDate)}</time>
            </li>
          ))}
        </ul>
      ) : (
        <p className="signal-empty">
          No hay lanzamientos futuros fechados entre tus juegos importados.
        </p>
      )}
    </SignalBlock>
  );
}

function EarlyAccessBlock({
  available,
  observations,
  events,
  loading,
}: {
  available: boolean;
  observations: number;
  events: DiscoveryEvent[];
  loading: boolean;
}) {
  return (
    <SignalBlock
      icon={IconTimelineEvent}
      title="Cambios de Early Access"
      headingId="signal-early-access-title"
      status={
        loading
          ? "Contrastando cambios"
          : available
            ? `${observations} observaciones guardadas`
            : "Fuente no disponible"
      }
      state={available ? "ready" : "unavailable"}
    >
      <p className="signal-block__note">
        {available
          ? events.length
            ? `${events.length} cambios comparados con observaciones anteriores.`
            : "Hay observaciones reales, pero aún no se ha detectado ningún cambio."
          : "Se activará tras guardar dos consultas reales comparables del mismo juego."}
      </p>
      {loading ? (
        <SignalSkeleton lines={2} />
      ) : events.length ? (
        <ul className="signal-list" aria-label="Historial de cambios verificados">
          {events.map((event) => (
            <li key={event.id}>
              <div>
                <strong>{event.title}</strong>
                <span>{eventDescription(event)}</span>
              </div>
              <time dateTime={event.observedAt}>{formatDate(event.observedAt)}</time>
            </li>
          ))}
        </ul>
      ) : (
        <p className="signal-empty">
          Cuando dos consultas reales del mismo juego difieran, el cambio aparecerá fechado aquí.
        </p>
      )}
    </SignalBlock>
  );
}

function DismissedBlock({
  items,
  loading,
  restoring,
  onRestore,
}: {
  items: DismissedRecommendation[];
  loading: boolean;
  restoring: boolean;
  onRestore: (id: string) => void;
}) {
  return (
    <SignalBlock
      icon={IconHistory}
      title="Recomendaciones descartadas"
      headingId="signal-dismissed-title"
      status={loading ? "Cargando historial" : `${items.length} guardadas`}
      state={items.length ? "ready" : "unavailable"}
    >
      {loading ? (
        <SignalSkeleton lines={2} />
      ) : items.length ? (
        <ul className="signal-list dismissed-list" aria-label="Historial de descartes">
          {items.map((item) => (
            <li key={item.id}>
              <div>
                <strong>{item.title}</strong>
                <span>
                  Descartado {formatDate(item.createdAt)}
                  {item.durationMinutes ? ` · ${item.durationMinutes} min` : ""}
                </span>
              </div>
              <Button
                size="xs"
                variant="outline"
                disabled={restoring}
                onClick={() => onRestore(item.id)}
              >
                <IconRestore /> Volver a considerar
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="signal-empty">
          Cuando descartes una elección, aparecerá aquí y podrás restaurarla.
        </p>
      )}
    </SignalBlock>
  );
}

function RecoverableError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className="discovery-error" role="alert">
      <IconRefresh aria-hidden="true" />
      <div className="discovery-error__copy">
        <strong>No se pudieron cargar los datos</strong>
        <span>{message}</span>
      </div>
      <Button size="sm" variant="outline" onClick={onRetry}>
        <IconRefresh /> Reintentar
      </Button>
    </div>
  );
}

function radarCount(view: RadarView, data: DiscoverySnapshot | undefined, tracked: number): number {
  if (view === "tracking") return tracked;
  if (view === "reminders") return data?.reminders.length ?? 0;
  if (view === "forgotten") return data?.forgotten.length ?? 0;
  return data?.almostFinished.length ?? 0;
}

function radarMetadata(game: GameSummary, view: Exclude<RadarView, "reminders">): string {
  if (view === "forgotten")
    return game.lastPlayedAt
      ? `Última sesión: ${formatDate(game.lastPlayedAt)}`
      : "Nunca jugado · importado hace más de 180 días";
  if (view === "almost")
    return `${game.progress}% completado · ${formatPlaytime(game.playtimeMinutes)} jugados`;
  return `${game.isEarlyAccess ? "Early Access · " : ""}Última sesión: ${formatDate(game.lastPlayedAt)}`;
}

function eventDescription(event: DiscoveryEvent): string {
  if (event.kind === "early_access_changed") {
    return event.currentValue === "released" ? "Salió de Early Access" : "Cambió a Early Access";
  }
  return `Fecha de lanzamiento: ${event.previousValue ?? "sin fecha"} → ${event.currentValue ?? "sin fecha"}`;
}

function formatReminderDate(value: string): string {
  const date = new Date(value);
  const overdue = date.getTime() < Date.now();
  return `${overdue ? "Vencido" : "Programado"}: ${new Intl.DateTimeFormat("es-ES", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date)}`;
}

function addDays(date: Date, days: number): Date {
  const result = new Date(date);
  result.setDate(result.getDate() + days);
  return result;
}

function durationLabel(value: string): string {
  if (value === "30") return "30 minutos";
  if (value === "120") return "2 horas";
  return "1 hora";
}

function moodLabel(value: string): string {
  return (
    {
      any: "Cualquiera",
      relaxed: "Relajada",
      focused: "Concentrada",
      competitive: "Competitiva",
      story: "Narrativa",
    }[value] ?? "Cualquiera"
  );
}
