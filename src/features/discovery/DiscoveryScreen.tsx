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
import { useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { EmptyState } from "@/components/common/EmptyState";
import { LoadingState } from "@/components/common/LoadingState";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  AppBootstrap,
  DiscoveryEvent,
  DiscoverySnapshot,
  GameReminder,
  GameSummary,
  NewsRefreshReport,
  OfficialPublication,
  Recommendation,
  RelatedRelease,
} from "@/lib/types";
import "./discovery.css";

type RadarView = "tracking" | "reminders" | "forgotten" | "almost";

const radarViews: { id: RadarView; label: string; icon: typeof IconEye }[] = [
  { id: "tracking", label: "Seguimiento", icon: IconEye },
  { id: "reminders", label: "Recordatorios", icon: IconBell },
  { id: "forgotten", label: "Olvidados", icon: IconHistory },
  { id: "almost", label: "Casi terminados", icon: IconCheck },
];

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

  if (loading && !bootstrap) return <LoadingState label="Cargando descubrimiento" />;
  return (
    <section className="discovery-screen" aria-labelledby="discovery-title">
      <span className="sr-only" role="status" aria-live="polite">
        {announcement}
      </span>
      <header className="screen-heading discovery-heading">
        <div>
          <p className="eyebrow">SEGUIMIENTO Y DESCUBRIMIENTO</p>
          <h1 id="discovery-title">Qué jugar ahora</h1>
          <p>Decisiones explicables con tu progreso, tus datos y el tiempo que tienes hoy.</p>
        </div>
        <div className="discovery-heading__metric">
          <IconEye aria-hidden="true" />
          <span>EN SEGUIMIENTO</span>
          <strong>{bootstrap?.stats.trackedGames ?? 0}</strong>
        </div>
      </header>

      <div className="discovery-scroll">
        <section className="decision-workbench" aria-labelledby="decision-title">
          <div className="decision-controls">
            <div>
              <p className="eyebrow">ELECCIÓN ASISTIDA</p>
              <h2 id="decision-title">Elige por mí</h2>
              <p>
                La explicación muestra qué datos reales influyeron. Si falta un dato, Vindexa lo
                dirá.
              </p>
            </div>
            <div className="decision-field">
              <span>
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
              <span>
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
              onClick={() => recommendation.mutate()}
              disabled={recommendation.isPending || (bootstrap?.stats.totalGames ?? 0) === 0}
            >
              {recommendation.isPending ? <IconLoader2 className="is-spinning" /> : <IconWand />}
              Elige por mí
            </Button>
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
          <div className="section-heading radar-panel__heading">
            <div>
              <p className="eyebrow">RADAR PERSONAL</p>
              <h2 id="radar-title">Vuelve a lo que importa</h2>
            </div>
            <span>Criterios visibles y editables desde cada ficha</span>
          </div>
          <div className="radar-tabs" role="tablist" aria-label="Vistas del radar">
            {radarViews.map(({ id, label, icon: ViewIcon }) => {
              const count = radarCount(id, discovery.data, tracked.data?.total ?? 0);
              return (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={radarView === id}
                  aria-controls="radar-content"
                  onClick={() => setRadarView(id)}
                >
                  <ViewIcon aria-hidden="true" />
                  <span>{label}</span>
                  <strong>{count}</strong>
                </button>
              );
            })}
          </div>
          <div id="radar-content" role="tabpanel" className="radar-content">
            {discovery.isPending || tracked.isPending ? (
              <LoadingState label="Cargando radar personal" />
            ) : discovery.isError || tracked.isError ? (
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
              <RadarEmpty view={radarView} />
            )}
          </div>
        </section>

        <section className="signals-panel" aria-labelledby="signals-title">
          <div className="section-heading">
            <div>
              <p className="eyebrow">CAMBIOS VERIFICABLES</p>
              <h2 id="signals-title">Señales de Steam y tu biblioteca</h2>
            </div>
            <span>
              {discovery.data?.capabilities.metadataObservations ?? 0} observaciones guardadas
            </span>
          </div>
          {discovery.isPending ? (
            <LoadingState label="Contrastando cambios" />
          ) : discovery.isError ? (
            <RecoverableError
              message={getErrorMessage(discovery.error)}
              onRetry={() => void discovery.refetch()}
            />
          ) : (
            <div className="signals-grid">
              <SignalCard
                icon={IconTimelineEvent}
                title="Cambios de Early Access"
                state={
                  discovery.data?.capabilities.earlyAccessHistoryAvailable ? "ready" : "unavailable"
                }
                message={
                  discovery.data?.capabilities.earlyAccessHistoryAvailable
                    ? discovery.data.events.length
                      ? `${discovery.data.events.length} cambios comparados con observaciones anteriores.`
                      : "Hay observaciones reales, pero aún no se ha detectado ningún cambio."
                    : "Se activará tras guardar dos consultas reales comparables del mismo juego."
                }
              />
              <OfficialPublicationsCard
                items={discovery.data?.officialPublications ?? []}
                trackedGames={discovery.data?.capabilities.trackedNewsGames ?? 0}
                refresh={newsRefresh}
              />
              <RelatedReleasesCard items={discovery.data?.relatedReleases ?? []} />
              <UpcomingCard items={discovery.data?.upcoming ?? []} />
            </div>
          )}
          {!!discovery.data?.events.length && <EventList items={discovery.data.events} />}
        </section>

        <section className="dismissed-panel" aria-labelledby="dismissed-title">
          <div className="section-heading">
            <div>
              <p className="eyebrow">HISTORIAL</p>
              <h2 id="dismissed-title">Recomendaciones descartadas</h2>
            </div>
            <span>{discovery.data?.dismissedRecommendations.length ?? 0} guardadas</span>
          </div>
          {discovery.isPending ? (
            <LoadingState label="Cargando historial" />
          ) : discovery.data?.dismissedRecommendations.length ? (
            <div className="dismissed-list">
              {discovery.data.dismissedRecommendations.map((item) => (
                <article key={item.id}>
                  <Artwork appId={item.appId} src={item.iconUrl} title={item.title} kind="icon" />
                  <div>
                    <strong>{item.title}</strong>
                    <span>
                      Descartado {formatDate(item.createdAt)}
                      {item.durationMinutes ? ` · ${item.durationMinutes} min` : ""}
                    </span>
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={restore.isPending}
                    onClick={() => restore.mutate(item.id)}
                  >
                    <IconRestore /> Volver a considerar
                  </Button>
                </article>
              ))}
            </div>
          ) : (
            <EmptyState
              compact
              icon={IconHistory}
              title="No has descartado recomendaciones"
              description="Cuando descartes una elección, aparecerá aquí y podrás restaurarla."
            />
          )}
        </section>
      </div>
    </section>
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
            <strong>Una decisión explicable</strong>
            <span>Vindexa seleccionará un único título y mostrará por qué encaja.</span>
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
  if (recommendation.isPending) return <LoadingState label="Buscando una elección compatible" />;
  if (recommendation.isError) {
    return (
      <RecoverableError
        message={getErrorMessage(recommendation.error)}
        onRetry={() => recommendation.mutate()}
      />
    );
  }
  const result = recommendation.data;
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
        <p className="eyebrow">RECOMENDACIÓN DE TU BIBLIOTECA</p>
        <h2>{result.game.title}</h2>
        <div className="decision-reasons">
          {result.reasons.map((reason) => (
            <Badge key={reason}>{reason}</Badge>
          ))}
        </div>
        <span>
          {formatPlaytime(result.game.playtimeMinutes)} jugados · {result.game.progress}% completado
        </span>
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
    <div className="radar-list">
      {items.map((game) => (
        <article key={game.appId}>
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
          <div className="radar-list__progress">
            <Progress
              value={game.progress}
              aria-label={`Progreso de ${game.title}: ${game.progress}%`}
            />
            <span>{game.progress}%</span>
          </div>
          {view !== "tracking" && (
            <Button size="sm" variant="outline" disabled={reminding} onClick={() => onRemind(game)}>
              <IconBellPlus /> Recordarme
            </Button>
          )}
          {view === "tracking" && (
            <Badge variant={game.installed ? "default" : "outline"}>
              {game.installed ? "Instalado" : "No instalado"}
            </Badge>
          )}
        </article>
      ))}
    </div>
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
    <div className="reminder-list">
      {items.map((item) => (
        <article key={item.id}>
          <Artwork appId={item.appId} src={item.iconUrl} title={item.title} kind="icon" />
          <div>
            <strong>{item.title}</strong>
            <span>
              {item.note || "Sin nota"} · {formatReminderDate(item.dueAt)}
            </span>
          </div>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => onSnooze(item)}>
            <IconCalendarEvent /> +7 días
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => onComplete(item.id)}>
            <IconCheck /> Hecho
          </Button>
        </article>
      ))}
    </div>
  );
}

function SignalCard({
  icon: SignalIcon,
  title,
  state,
  message,
}: {
  icon: typeof IconBolt;
  title: string;
  state: "ready" | "unavailable";
  message: string;
}) {
  return (
    <article className="signal-card" data-state={state}>
      <SignalIcon aria-hidden="true" />
      <div>
        <strong>{title}</strong>
        <span>{state === "ready" ? "Datos comparables" : "Fuente no disponible"}</span>
      </div>
      <p>{message}</p>
    </article>
  );
}

function OfficialPublicationsCard({
  items,
  trackedGames,
  refresh,
}: {
  items: OfficialPublication[];
  trackedGames: number;
  refresh: UseQueryResult<NewsRefreshReport, Error>;
}) {
  const unavailable = trackedGames === 0;
  const status = refresh.isFetching
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
    <article
      className="signal-card signal-card--publications"
      data-state={unavailable ? "unavailable" : "ready"}
    >
      <IconBolt aria-hidden="true" />
      <div>
        <strong>Publicaciones oficiales recientes</strong>
        <span>{status}</span>
      </div>
      <Button
        className="signal-card__refresh"
        size="sm"
        variant="ghost"
        disabled={unavailable || refresh.isFetching}
        onClick={() => void refresh.refetch()}
      >
        {refresh.isFetching ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}
        Actualizar
      </Button>
      <p>
        Feed público oficial de Steam. Este método no expone una señal de importancia: Vindexa
        muestra cada publicación con su fuente y fecha, sin clasificarla.
      </p>
      {unavailable ? (
        <div className="signal-card__empty">
          Sigue al menos un juego para consultar su feed sin usar tu Web API Key.
        </div>
      ) : refresh.isError && !items.length ? (
        <div className="signal-card__inline-error" role="alert">
          <span>{getErrorMessage(refresh.error)}</span>
          <button type="button" onClick={() => void refresh.refetch()}>
            Reintentar
          </button>
        </div>
      ) : refresh.isFetching && !items.length ? (
        <div className="signal-card__empty" role="status">
          Contrastando el feed oficial…
        </div>
      ) : items.length ? (
        <ul className="signal-publications" aria-label="Publicaciones verificadas de Steam">
          {items.slice(0, 3).map((item) => (
            <li key={`${item.appId}-${item.gid}`}>
              <div>
                <strong>{item.title}</strong>
                <span>{item.gameTitle}</span>
              </div>
              <small>
                {item.feedLabel} · {formatDate(item.publishedAt)}
              </small>
              {!!item.contentPreview && <p>{item.contentPreview}</p>}
            </li>
          ))}
        </ul>
      ) : (
        <div className="signal-card__empty">
          Steam no devolvió publicaciones recientes para los juegos seguidos.
        </div>
      )}
    </article>
  );
}

function RelatedReleasesCard({ items }: { items: RelatedRelease[] }) {
  return (
    <article
      className="signal-card signal-card--related"
      data-state={items.length ? "ready" : "unavailable"}
    >
      <IconSparkles aria-hidden="true" />
      <div>
        <strong>Lanzamientos relacionados</strong>
        <span>{items.length ? `${items.length} relaciones verificadas` : "Sin coincidencias"}</span>
      </div>
      <p>
        Solo títulos importados con fecha real y la misma empresa normalizada. No se infieren
        relaciones mediante IA.
      </p>
      {items.length ? (
        <ul className="related-release-list" aria-label="Lanzamientos relacionados verificables">
          {items.slice(0, 3).map((item) => (
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
        <div className="signal-card__empty">
          Aún no hay un lanzamiento futuro que coincida exactamente con un desarrollador o editor de
          un juego que hayas usado.
        </div>
      )}
    </article>
  );
}

function UpcomingCard({ items }: { items: GameSummary[] }) {
  return (
    <article
      className="signal-card signal-card--upcoming"
      data-state={items.length ? "ready" : "unavailable"}
    >
      <IconCalendarEvent aria-hidden="true" />
      <div>
        <strong>Próximos de tu biblioteca</strong>
        <span>{items.length ? `${items.length} fechas reales` : "Sin fechas futuras"}</span>
      </div>
      {items.length ? (
        <ul>
          {items.slice(0, 3).map((game) => (
            <li key={game.appId}>
              <span>{game.title}</span>
              <time>{formatDate(game.releaseDate)}</time>
            </li>
          ))}
        </ul>
      ) : (
        <p>No hay lanzamientos futuros fechados entre tus juegos importados.</p>
      )}
    </article>
  );
}

function EventList({ items }: { items: DiscoveryEvent[] }) {
  return (
    <ul className="event-list" aria-label="Historial de cambios verificados">
      {items.map((event) => (
        <li key={event.id}>
          <Artwork appId={event.appId} src={event.iconUrl} title={event.title} kind="icon" />
          <div>
            <strong>{event.title}</strong>
            <span>{eventDescription(event)}</span>
          </div>
          <time>{formatDate(event.observedAt)}</time>
        </li>
      ))}
    </ul>
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

function RadarEmpty({ view }: { view: Exclude<RadarView, "reminders"> }) {
  const copy = {
    tracking: {
      title: "No sigues ningún juego",
      description: "Activa “Seguir actualizaciones” desde la ficha de un título.",
    },
    forgotten: {
      title: "No hay juegos olvidados",
      description: "Aquí aparecen los no jugados en 180 días y los inactivos durante un año.",
    },
    almost: {
      title: "No hay juegos casi terminados",
      description: "Esta vista usa únicamente progreso propio entre 75 % y 99 %.",
    },
  }[view];
  return <EmptyState compact icon={IconEye} title={copy.title} description={copy.description} />;
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
