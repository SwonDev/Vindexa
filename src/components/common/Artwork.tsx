import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { ARTWORK_CACHE_CLEARED_EVENT } from "@/lib/artwork-cache-events";
import { initials } from "@/lib/format";
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type ArtworkKind = "cover" | "header" | "icon" | "hero";

interface ArtworkProps {
  appId?: number | undefined;
  src?: string | undefined;
  title: string;
  className?: string | undefined;
  kind?: ArtworkKind | undefined;
  priority?: boolean | undefined;
}

const NEGATIVE_CACHE_MS = 60_000;
const MAX_MEMORY_ENTRIES = 10_000;
const localArtwork = new Map<string, string>();
/** Tamaño real del archivo cacheado, para reservar el hueco sin salto. */
const artworkSize = new Map<string, { width: number; height: number }>();
const pendingArtwork = new Map<string, Promise<string>>();
const unavailableArtwork = new Map<string, number>();
let cacheGeneration = 0;

function resetArtworkMemoryCache() {
  cacheGeneration += 1;
  localArtwork.clear();
  artworkSize.clear();
  pendingArtwork.clear();
  unavailableArtwork.clear();
}

if (typeof window !== "undefined") {
  window.addEventListener(ARTWORK_CACHE_CLEARED_EVENT, resetArtworkMemoryCache);
}

function storeBounded<T>(cache: Map<string, T>, key: string, value: T) {
  if (!cache.has(key) && cache.size >= MAX_MEMORY_ENTRIES) {
    const oldest = cache.keys().next().value;
    if (oldest) cache.delete(oldest);
  }
  cache.set(key, value);
}

function requestLocalArtwork(
  appId: number,
  kind: ArtworkKind,
  sourceUrl: string,
  cacheKey: string,
): Promise<string> {
  const local = localArtwork.get(cacheKey);
  if (local) return Promise.resolve(local);

  const unavailableUntil = unavailableArtwork.get(cacheKey) ?? 0;
  if (unavailableUntil > Date.now()) {
    return Promise.reject(new Error("artwork_negative_cache"));
  }
  unavailableArtwork.delete(cacheKey);

  const pending = pendingArtwork.get(cacheKey);
  if (pending) return pending;

  const generation = cacheGeneration;
  const request = api
    .cacheGameArt(appId, kind, sourceUrl)
    .then(({ localPath, width, height }) => {
      if (generation !== cacheGeneration) {
        throw new Error("artwork_cache_reset");
      }
      const localUrl = convertFileSrc(localPath);
      storeBounded(localArtwork, cacheKey, localUrl);
      if (width && height) storeBounded(artworkSize, cacheKey, { width, height });
      unavailableArtwork.delete(cacheKey);
      return localUrl;
    })
    .catch((error: unknown) => {
      if (generation === cacheGeneration) {
        storeBounded(unavailableArtwork, cacheKey, Date.now() + NEGATIVE_CACHE_MS);
      }
      throw error;
    })
    .finally(() => {
      if (pendingArtwork.get(cacheKey) === request) {
        pendingArtwork.delete(cacheKey);
      }
    });
  pendingArtwork.set(cacheKey, request);
  return request;
}

function accessibleLabel(kind: ArtworkKind, title: string): string {
  if (kind === "cover") return `Carátula de ${title}`;
  if (kind === "header") return `Cabecera de ${title}`;
  if (kind === "hero") return `Arte principal de ${title}`;
  return `Icono de ${title}`;
}

/**
 * Cuántas peticiones de precarga viajan a la vez.
 *
 * Cada una cruza el puente hacia Rust; sin límite, precargar una página entera
 * ahogaría las peticiones de las tarjetas que la persona **sí** está mirando,
 * que son las que importan.
 */
const PREFETCH_CONCURRENCY = 6;
let prefetchInFlight = 0;
const prefetchQueue: (() => void)[] = [];

function runNextPrefetch() {
  if (prefetchInFlight >= PREFETCH_CONCURRENCY) return;
  const next = prefetchQueue.shift();
  if (!next) return;
  prefetchInFlight += 1;
  next();
}

/**
 * Adelanta a la caché las carátulas que aún no están resueltas.
 *
 * La tarjeta pide su imagen al montarse, y con una lista virtualizada eso
 * ocurre justo cuando entra en pantalla: por eso las portadas «aparecían» al
 * desplazarse. Resolviéndolas antes, cuando la tarjeta se monta la URL local ya
 * está en memoria y se pinta en el primer fotograma, sin petición ni espera.
 *
 * Lo ya resuelto o en vuelo no se vuelve a pedir, y los fallos se ignoran a
 * propósito: esto es una mejora de tiempos, no una fuente de errores. Si algo
 * falla aquí, la tarjeta lo reintentará por su cuenta al montarse.
 */
export function prefetchArtwork(
  entries: readonly { appId: number; src?: string | undefined }[],
  kind: ArtworkKind = "cover",
): void {
  for (const entry of entries) {
    if (!entry.appId || !entry.src) continue;
    const cacheKey = `${entry.appId}:${kind}:${entry.src}`;
    if (localArtwork.has(cacheKey) || pendingArtwork.has(cacheKey)) continue;
    if ((unavailableArtwork.get(cacheKey) ?? 0) > Date.now()) continue;
    const { appId, src } = entry;
    prefetchQueue.push(() => {
      requestLocalArtwork(appId, kind, src, cacheKey)
        .catch(() => undefined)
        .finally(() => {
          prefetchInFlight -= 1;
          runNextPrefetch();
        });
    });
  }
  for (let slot = prefetchInFlight; slot < PREFETCH_CONCURRENCY; slot += 1) {
    runNextPrefetch();
  }
}

export function Artwork({
  appId,
  src,
  title,
  className,
  kind = "cover",
  priority = false,
}: ArtworkProps) {
  const elementRef = useRef<HTMLElement | null>(null);
  const cacheKey = appId && src ? `${appId}:${kind}:${src}` : undefined;
  const isPriority = priority || kind === "header" || kind === "hero";
  const [isVisible, setIsVisible] = useState(isPriority);
  const [observedCacheGeneration, setObservedCacheGeneration] = useState(cacheGeneration);
  const [resolvedSrc, setResolvedSrc] = useState<string | undefined>(() =>
    cacheKey ? localArtwork.get(cacheKey) : src,
  );
  const [imageFailed, setImageFailed] = useState(false);
  const [retryNonce, setRetryNonce] = useState(0);
  const retryTimer = useRef<number | undefined>(undefined);

  // Un fallo (red o CDN) nunca es terminal: al expirar la caché negativa se
  // reprograma un intento sin desmontar la tarjeta.
  const scheduleRetry = (delayMs: number) => {
    window.clearTimeout(retryTimer.current);
    retryTimer.current = window.setTimeout(
      () => setRetryNonce((nonce) => nonce + 1),
      Math.max(250, delayMs),
    );
  };

  useEffect(
    () => () => {
      window.clearTimeout(retryTimer.current);
    },
    [],
  );

  useEffect(() => {
    const handleCacheClear = () => {
      setImageFailed(false);
      setResolvedSrc(undefined);
      setObservedCacheGeneration(cacheGeneration);
    };
    window.addEventListener(ARTWORK_CACHE_CLEARED_EVENT, handleCacheClear);
    return () => window.removeEventListener(ARTWORK_CACHE_CLEARED_EVENT, handleCacheClear);
  }, []);

  useEffect(() => {
    if (isPriority) {
      setIsVisible(true);
      return;
    }
    const element = elementRef.current;
    if (!element || typeof IntersectionObserver === "undefined") {
      setIsVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setIsVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "240px 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [isPriority]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: retryNonce es una señal de reprogramación deliberada y scheduleRetry solo toca refs/estado estable
  useEffect(() => {
    if (observedCacheGeneration !== cacheGeneration) return;
    setImageFailed(false);
    setResolvedSrc(cacheKey ? localArtwork.get(cacheKey) : src);
    if (!isVisible || !appId || !src || !cacheKey) return;

    let active = true;
    void requestLocalArtwork(appId, kind, src, cacheKey)
      .then((localUrl) => {
        if (active) setResolvedSrc(localUrl);
      })
      .catch(() => {
        if (!active) return;
        setResolvedSrc(undefined);
        const retryIn = (unavailableArtwork.get(cacheKey) ?? 0) - Date.now();
        if (retryIn > 0) scheduleRetry(retryIn + 75);
      });
    return () => {
      active = false;
    };
  }, [appId, cacheKey, isVisible, kind, observedCacheGeneration, retryNonce, src]);

  if (!resolvedSrc || imageFailed) {
    return (
      <div
        ref={(element) => {
          elementRef.current = element;
        }}
        className={cn("artwork artwork--fallback", `artwork--${kind}`, className)}
        data-loading={Boolean(appId && src && isVisible && !imageFailed)}
        role="img"
        aria-label={accessibleLabel(kind, title)}
      >
        {/* El hueco de una carátula ausente tiene que decir de qué juego se
            trata: un rectángulo mudo obliga a abrir la ficha para saberlo. */}
        <span aria-hidden="true">{initials(title)}</span>
        {kind === "cover" && <em className="artwork__title">{title}</em>}
      </div>
    );
  }

  // Las dimensiones reales llegan del backend: declararlas evita que la
  // maquetación salte cuando el archivo termina de decodificarse.
  const intrinsic = cacheKey ? artworkSize.get(cacheKey) : undefined;
  return (
    <img
      ref={(element) => {
        elementRef.current = element;
      }}
      className={cn("artwork", `artwork--${kind}`, className)}
      src={resolvedSrc}
      width={intrinsic?.width}
      height={intrinsic?.height}
      alt={accessibleLabel(kind, title)}
      loading={isPriority ? "eager" : "lazy"}
      decoding="async"
      fetchPriority={isPriority ? "high" : "auto"}
      onError={() => {
        if (cacheKey) {
          localArtwork.delete(cacheKey);
          storeBounded(unavailableArtwork, cacheKey, Date.now() + NEGATIVE_CACHE_MS);
        }
        setImageFailed(true);
        scheduleRetry(NEGATIVE_CACHE_MS + 75);
      }}
    />
  );
}
