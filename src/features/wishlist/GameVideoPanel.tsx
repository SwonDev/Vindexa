import {
  IconAlertTriangle,
  IconArrowDown,
  IconArrowUp,
  IconLoader2,
  IconPlayerPlayFilled,
  IconPlus,
  IconTrash,
  IconVideo,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { PressableSurface, SegmentedControl, ShimmerSkeleton } from "@/components/motion";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  groupVideosByKind,
  moveWithin,
  VIDEO_KINDS,
  videoKindLabel,
} from "@/features/wishlist/wishlist-model";
import { api, getErrorMessage } from "@/lib/tauri";
import type { GameVideo, GameVideoRef, SaveGameVideoInput, VideoKind } from "@/lib/types";

type KindFilter = VideoKind | "all";

const FILTER_OPTIONS = [
  { value: "all" as const, label: "Todos" },
  ...VIDEO_KINDS.map((kind) => ({ value: kind.id, label: kind.label })),
];

/**
 * Vídeos guardados de un juego.
 *
 * Contrato de seguridad que gobierna todo este archivo: **la URL del `iframe`
 * la construye Rust**, no esta pantalla. `GameVideo.embedUrl` llega ya validada
 * y apuntando a `youtube-nocookie.com`, el único origen que la política de
 * contenido de la aplicación permite incrustar; aquí se usa tal cual o no se
 * usa. Concatenar un identificador con una plantilla desde el frontal sería
 * convertir un campo de texto en un vector de inyección de marcos.
 *
 * Por el mismo motivo no se pinta ninguna miniatura de `i.ytimg.com`: el
 * backend las rechaza a propósito, porque una miniatura remota es una petición
 * a Google hecha antes de que nadie haya decidido reproducir nada. El cartel es
 * la portada del propio juego, que ya está en la caché local, y el marco no
 * existe en el DOM hasta que se pulsa reproducir.
 */
export function GameVideoPanel({
  appId,
  title,
  headerUrl,
  coverUrl,
}: {
  appId: number;
  title: string;
  headerUrl?: string | undefined;
  coverUrl?: string | undefined;
}) {
  const queryClient = useQueryClient();
  const formId = useId();
  const [filter, setFilter] = useState<KindFilter>("all");
  const [source, setSource] = useState("");
  const [kind, setKind] = useState<VideoKind>("gameplay");
  const [videoTitle, setVideoTitle] = useState("");
  const [channel, setChannel] = useState("");
  const [message, setMessage] = useState<{ tone: "info" | "error"; text: string }>();

  const videos = useQuery({
    queryKey: ["game-videos", appId],
    queryFn: () => api.listGameVideos(appId),
    staleTime: 60_000,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["game-videos", appId] });

  const add = useMutation({
    mutationFn: (input: SaveGameVideoInput) => api.saveGameVideo(input),
    onSuccess: (video) => {
      setSource("");
      setVideoTitle("");
      setChannel("");
      setMessage({ tone: "info", text: `Guardado: ${video.title || video.videoId}.` });
      void invalidate();
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const remove = useMutation({
    mutationFn: (video: GameVideo) => api.deleteGameVideo(appId, video.provider, video.videoId),
    onSuccess: (_data, video) => {
      setMessage({ tone: "info", text: `Se quitó «${video.title || video.videoId}».` });
      void invalidate();
    },
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const reorder = useMutation({
    mutationFn: ({ kind: group, ordered }: { kind: VideoKind; ordered: GameVideoRef[] }) =>
      api.reorderGameVideos(appId, group, ordered),
    onSuccess: () => void invalidate(),
    onError: (cause) => setMessage({ tone: "error", text: getErrorMessage(cause) }),
  });

  const submit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = source.trim();
    if (!trimmed) {
      setMessage({ tone: "error", text: "Pega la URL del vídeo o su identificador." });
      return;
    }
    // El identificador no se extrae aquí: lo hace Rust, que además es quien
    // decide si el vídeo puede incrustarse. Duplicar esa lógica en el frontal
    // crearía dos verdades y la de aquí sería la insegura.
    const input: SaveGameVideoInput = { appId, video: trimmed, kind };
    const cleanTitle = videoTitle.trim();
    const cleanChannel = channel.trim();
    add.mutate({
      ...input,
      ...(cleanTitle ? { title: cleanTitle } : {}),
      ...(cleanChannel ? { channel: cleanChannel } : {}),
    });
  };

  const all = videos.data ?? [];
  const visible = filter === "all" ? all : all.filter((video) => video.kind === filter);
  const groups = groupVideosByKind(visible);

  const moveVideo = (group: VideoKind, index: number, direction: -1 | 1) => {
    const ordered = all
      .filter((video) => video.kind === group)
      .sort((left, right) => left.position - right.position);
    const next = moveWithin(ordered, index, index + direction);
    if (next === ordered) return;
    reorder.mutate({
      kind: group,
      ordered: next.map((video) => ({ provider: video.provider, videoId: video.videoId })),
    });
  };

  return (
    <section className="video-panel" aria-label={`Vídeos de ${title}`}>
      <header className="video-panel__head">
        <h3 className="video-panel__title">
          <IconVideo aria-hidden="true" /> Vídeos guardados
          <data value={all.length}>{all.length.toLocaleString("es-ES")}</data>
        </h3>
        {all.length > 0 && (
          <SegmentedControl
            className="video-panel__filter"
            size="sm"
            label="Filtrar vídeos por tipo"
            options={FILTER_OPTIONS}
            value={filter}
            onValueChange={setFilter}
          />
        )}
      </header>

      <form className="video-form" onSubmit={submit}>
        <label className="video-form__field video-form__field--wide" htmlFor={`${formId}-source`}>
          <span>URL o identificador</span>
          <Input
            id={`${formId}-source`}
            value={source}
            placeholder="https://www.youtube.com/watch?v=…"
            onChange={(event) => setSource(event.currentTarget.value)}
          />
        </label>
        <label className="video-form__field" htmlFor={`${formId}-kind`}>
          <span>Tipo</span>
          <Select value={kind} onValueChange={(value) => setKind(value as VideoKind)}>
            <SelectTrigger id={`${formId}-kind`} aria-label="Tipo de vídeo">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {VIDEO_KINDS.map((option) => (
                <SelectItem key={option.id} value={option.id}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <label className="video-form__field" htmlFor={`${formId}-title`}>
          <span>Título (opcional)</span>
          <Input
            id={`${formId}-title`}
            value={videoTitle}
            onChange={(event) => setVideoTitle(event.currentTarget.value)}
          />
        </label>
        <label className="video-form__field" htmlFor={`${formId}-channel`}>
          <span>Canal (opcional)</span>
          <Input
            id={`${formId}-channel`}
            value={channel}
            onChange={(event) => setChannel(event.currentTarget.value)}
          />
        </label>
        <Button type="submit" size="sm" disabled={add.isPending}>
          {add.isPending ? <IconLoader2 className="is-spinning" /> : <IconPlus />} Guardar vídeo
        </Button>
      </form>

      {message ? (
        <p
          className="inline-notice video-panel__notice"
          data-kind={message.tone === "error" ? "error" : undefined}
          role={message.tone === "error" ? "alert" : "status"}
        >
          {message.tone === "error" && <IconAlertTriangle aria-hidden="true" />}
          <span>{message.text}</span>
        </p>
      ) : (
        <span className="sr-only" role="status" aria-live="polite" />
      )}

      {videos.isPending ? (
        <div className="video-grid" aria-busy="true">
          {Array.from({ length: 2 }, (_, slot) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: huecos idénticos sin identidad propia
            <div key={slot} className="video-card video-card--skeleton" aria-hidden="true">
              <ShimmerSkeleton aspectRatio="16 / 9" radiusPx={2} />
              <ShimmerSkeleton height={12} width="70%" radiusPx={2} />
              <ShimmerSkeleton height={10} width="45%" radiusPx={2} />
            </div>
          ))}
        </div>
      ) : videos.isError ? (
        <p className="video-panel__empty" role="alert">
          <strong>No se pudieron leer los vídeos</strong>
          {getErrorMessage(videos.error)}
          <Button variant="outline" size="sm" onClick={() => void videos.refetch()}>
            Reintentar
          </Button>
        </p>
      ) : !all.length ? (
        <p className="video-panel__empty">
          <strong>Todavía no hay ningún vídeo aquí</strong>
          Guarda aquí el vídeo que te convenció, para no volver a buscarlo cuando dudes si
          comprarlo.
        </p>
      ) : !visible.length ? (
        <p className="video-panel__empty" role="status">
          <strong>Ninguno de tipo «{videoKindLabel(filter as VideoKind)}»</strong>
          Cambia el filtro o guarda uno con ese tipo.
        </p>
      ) : (
        groups.map((group) => (
          <section
            className="video-group"
            key={group.kind}
            aria-label={`${group.label} (${group.items.length})`}
          >
            <h4 className="video-group__title">
              {group.label}
              <data value={group.items.length}>{group.items.length}</data>
            </h4>
            <div className="video-grid">
              {group.items.map((video, index) => (
                <VideoCard
                  key={`${video.provider}:${video.videoId}`}
                  video={video}
                  gameTitle={title}
                  gameAppId={appId}
                  posterUrl={headerUrl ?? coverUrl}
                  first={index === 0}
                  last={index === group.items.length - 1}
                  busy={reorder.isPending || remove.isPending}
                  onMove={(direction) => moveVideo(group.kind, index, direction)}
                  onRemove={() => remove.mutate(video)}
                />
              ))}
            </div>
          </section>
        ))
      )}
    </section>
  );
}

function VideoCard({
  video,
  gameTitle,
  gameAppId,
  posterUrl,
  first,
  last,
  busy,
  onMove,
  onRemove,
}: {
  video: GameVideo;
  gameTitle: string;
  gameAppId: number;
  posterUrl?: string | undefined;
  first: boolean;
  last: boolean;
  busy: boolean;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
}) {
  const [playing, setPlaying] = useState(false);
  const label = video.title || video.videoId;
  const embeddable = Boolean(video.embedUrl);

  return (
    <article className="video-card" data-playing={playing}>
      <div className="video-card__stage">
        {playing && video.embedUrl ? (
          <iframe
            className="video-card__frame"
            /* Valor devuelto por el backend, jamás compuesto aquí. */
            src={video.embedUrl}
            title={`Reproductor de ${label}`}
            allow="encrypted-media; picture-in-picture; fullscreen"
            allowFullScreen
            loading="lazy"
            referrerPolicy="no-referrer"
            sandbox="allow-scripts allow-same-origin allow-presentation"
          />
        ) : (
          <PressableSurface asChild liftPx={1} hoverScale={1.004}>
            <button
              type="button"
              className="video-card__poster"
              disabled={!embeddable}
              aria-label={
                embeddable
                  ? `Reproducir ${label}`
                  : `${label} no se puede incrustar: ábrelo en su plataforma`
              }
              onClick={() => setPlaying(true)}
            >
              {/* La carátula del juego, servida desde la caché local. Nunca una
                  miniatura remota: eso sería una petición a Google antes de que
                  nadie haya pulsado nada. */}
              <span className="video-card__art" aria-hidden="true">
                <Artwork
                  appId={gameAppId}
                  src={posterUrl}
                  title={gameTitle}
                  kind={posterUrl ? "header" : "cover"}
                />
              </span>
              <span className="video-card__play" aria-hidden="true">
                {embeddable ? <IconPlayerPlayFilled /> : <IconAlertTriangle />}
              </span>
              <span className="video-card__badge">{videoKindLabel(video.kind)}</span>
            </button>
          </PressableSurface>
        )}
      </div>
      <div className="video-card__body">
        <p className="video-card__title">{label}</p>
        <p className="video-card__meta">
          {video.channel || "Canal sin anotar"}
          <span aria-hidden="true">·</span>
          {video.provider === "youtube" ? "YouTube" : "Steam"}
          {!embeddable && <span className="video-card__warning">Sin reproducción incrustada</span>}
        </p>
      </div>
      <div className="video-card__actions">
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`Subir ${label}`}
          disabled={busy || first}
          onClick={() => onMove(-1)}
        >
          <IconArrowUp />
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`Bajar ${label}`}
          disabled={busy || last}
          onClick={() => onMove(1)}
        >
          <IconArrowDown />
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`Quitar ${label}`}
          disabled={busy}
          onClick={onRemove}
        >
          <IconTrash />
        </Button>
      </div>
    </article>
  );
}
