import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
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
const pendingArtwork = new Map<string, Promise<string>>();
const unavailableArtwork = new Map<string, number>();

function storeBounded<T>(cache: Map<string, T>, key: string, value: T) {
  if (!cache.has(key) && cache.size >= MAX_MEMORY_ENTRIES) {
    const oldest = cache.keys().next().value;
    if (oldest) cache.delete(oldest);
  }
  cache.set(key, value);
}

function requestLocalArtwork(appId: number, kind: ArtworkKind, cacheKey: string): Promise<string> {
  const local = localArtwork.get(cacheKey);
  if (local) return Promise.resolve(local);

  const unavailableUntil = unavailableArtwork.get(cacheKey) ?? 0;
  if (unavailableUntil > Date.now()) {
    return Promise.reject(new Error("artwork_negative_cache"));
  }
  unavailableArtwork.delete(cacheKey);

  const pending = pendingArtwork.get(cacheKey);
  if (pending) return pending;

  const request = api
    .cacheGameArt(appId, kind)
    .then(({ localPath }) => {
      const localUrl = convertFileSrc(localPath);
      storeBounded(localArtwork, cacheKey, localUrl);
      unavailableArtwork.delete(cacheKey);
      return localUrl;
    })
    .catch((error: unknown) => {
      storeBounded(unavailableArtwork, cacheKey, Date.now() + NEGATIVE_CACHE_MS);
      throw error;
    })
    .finally(() => {
      pendingArtwork.delete(cacheKey);
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

export function Artwork({
  appId,
  src,
  title,
  className,
  kind = "cover",
  priority = false,
}: ArtworkProps) {
  const elementRef = useRef<HTMLElement | null>(null);
  const cacheKey = appId ? `${appId}:${kind}` : undefined;
  const isPriority = priority || kind === "header" || kind === "hero";
  const [isVisible, setIsVisible] = useState(isPriority);
  const [resolvedSrc, setResolvedSrc] = useState<string | undefined>(() =>
    cacheKey ? localArtwork.get(cacheKey) : src,
  );
  const [imageFailed, setImageFailed] = useState(false);

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

  useEffect(() => {
    setImageFailed(false);
    setResolvedSrc(cacheKey ? localArtwork.get(cacheKey) : src);
    if (!isVisible || !appId || !src || !cacheKey) return;

    let active = true;
    void requestLocalArtwork(appId, kind, cacheKey)
      .then((localUrl) => {
        if (active) setResolvedSrc(localUrl);
      })
      .catch(() => {
        if (active) setResolvedSrc(undefined);
      });
    return () => {
      active = false;
    };
  }, [appId, cacheKey, isVisible, kind, src]);

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
        <span>{initials(title)}</span>
      </div>
    );
  }

  return (
    <img
      ref={(element) => {
        elementRef.current = element;
      }}
      className={cn("artwork", `artwork--${kind}`, className)}
      src={resolvedSrc}
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
      }}
    />
  );
}
