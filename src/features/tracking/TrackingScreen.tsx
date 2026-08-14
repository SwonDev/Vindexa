import {
  IconBolt,
  IconClock,
  IconDeviceGamepad2,
  IconLoader2,
  IconRadar,
  IconRefresh,
  IconWand,
} from "@tabler/icons-react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
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
import type { AppBootstrap } from "@/lib/types";

export function TrackingScreen({
  bootstrap,
  loading,
}: {
  bootstrap?: AppBootstrap;
  loading: boolean;
}) {
  const [duration, setDuration] = useState("60");
  const [mood, setMood] = useState("any");
  const tracked = useQuery({
    queryKey: ["games", "tracking"],
    queryFn: () => api.listGames({ tracking: true, sort: "lastPlayed", limit: 200 }),
  });
  const recommendation = useMutation({
    mutationFn: () => api.recommendGame(Number(duration), mood === "any" ? undefined : mood),
  });
  if (loading && !bootstrap) return <LoadingState label="Cargando seguimiento" />;
  return (
    <section className="tracking-screen">
      <header className="screen-heading">
        <div>
          <p className="eyebrow">SEGUIMIENTO Y DECISIÓN</p>
          <h1>Qué jugar ahora</h1>
          <p>
            Una recomendación explicable basada únicamente en tu biblioteca, tiempo disponible y
            planificación.
          </p>
        </div>
        <div className="tracking-count">
          <IconRadar />
          <span>EN SEGUIMIENTO</span>
          <strong>{bootstrap?.stats.trackedGames ?? 0}</strong>
        </div>
      </header>
      <div className="recommendation-workbench">
        <div className="recommendation-controls">
          <div>
            <IconClock />
            <span>TIEMPO DISPONIBLE</span>
            <Select value={duration} onValueChange={setDuration}>
              <SelectTrigger aria-label="Tiempo disponible">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="30">30 minutos</SelectItem>
                <SelectItem value="60">1 hora</SelectItem>
                <SelectItem value="120">2 horas</SelectItem>
                <SelectItem value="240">Tarde larga</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div>
            <IconBolt />
            <span>TIPO DE EXPERIENCIA</span>
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
            {recommendation.isPending ? <IconLoader2 className="is-spinning" /> : <IconWand />}{" "}
            Elige por mí
          </Button>
        </div>
        <div className="recommendation-result" aria-live="polite">
          {recommendation.isIdle ? (
            <div className="recommendation-placeholder">
              <IconWand />
              <div>
                <strong>Decide con restricciones reales</strong>
                <span>Vindexa prioriza tu progreso, planificación y tiempo disponible.</span>
              </div>
            </div>
          ) : recommendation.isError ? (
            <div className="recommendation-placeholder recommendation-placeholder--error">
              <IconRefresh />
              <div>
                <strong>No hay recomendación disponible</strong>
                <span>{getErrorMessage(recommendation.error)}</span>
              </div>
            </div>
          ) : recommendation.data ? (
            <>
              <Artwork
                appId={recommendation.data.game.appId}
                src={recommendation.data.game.headerUrl ?? recommendation.data.game.coverUrl}
                title={recommendation.data.game.title}
                kind="header"
              />
              <div className="recommendation-result__scrim" />
              <div className="recommendation-result__content">
                <p className="eyebrow">RECOMENDACIÓN DE TU BIBLIOTECA</p>
                <h2>{recommendation.data.game.title}</h2>
                <div>
                  {recommendation.data.reasons.map((reason) => (
                    <Badge key={reason}>{reason}</Badge>
                  ))}
                </div>
                <span>
                  {formatPlaytime(recommendation.data.game.playtimeMinutes)} jugados ·{" "}
                  {recommendation.data.game.progress}% completado
                </span>
                <Button
                  size="sm"
                  onClick={() => api.launchGame(recommendation.data.game.appId)}
                  disabled={!recommendation.data.game.installed}
                >
                  <IconDeviceGamepad2 />{" "}
                  {recommendation.data.game.installed ? "Jugar ahora" : "No instalado"}
                </Button>
              </div>
            </>
          ) : null}
        </div>
      </div>
      <section className="tracked-library">
        <div className="section-heading">
          <div>
            <p className="eyebrow">RADAR PERSONAL</p>
            <h2>Juegos en seguimiento</h2>
          </div>
          <span>{tracked.data?.total ?? 0} títulos</span>
        </div>
        {tracked.isPending ? (
          <LoadingState label="Cargando seguimiento" />
        ) : tracked.data?.items.length ? (
          <div className="tracked-list">
            {tracked.data.items.map((game) => (
              <article key={game.appId}>
                <Artwork
                  appId={game.appId}
                  src={game.iconUrl ?? game.coverUrl}
                  title={game.title}
                  kind="icon"
                />
                <div>
                  <strong>{game.title}</strong>
                  <span>
                    {game.isEarlyAccess ? "Early Access · " : ""}Última sesión:{" "}
                    {formatDate(game.lastPlayedAt)}
                  </span>
                </div>
                <div>
                  <Progress
                    value={game.progress}
                    aria-label={`Progreso de ${game.title}: ${game.progress}%`}
                  />
                  <span>{game.progress}%</span>
                </div>
                <Badge variant={game.installed ? "default" : "outline"}>
                  {game.installed ? "Instalado" : "No instalado"}
                </Badge>
              </article>
            ))}
          </div>
        ) : (
          <EmptyState
            compact
            icon={IconRadar}
            title="No sigues ningún juego"
            description="Activa “Seguir actualizaciones” desde la ficha de cualquier título."
          />
        )}
      </section>
    </section>
  );
}
