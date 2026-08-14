export const ARTWORK_CACHE_CLEARED_EVENT = "vindexa:artwork-cache-cleared";

export function notifyArtworkCacheCleared(): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(ARTWORK_CACHE_CLEARED_EVENT));
  }
}
