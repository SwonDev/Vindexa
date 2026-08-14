import type { CollectionSummary, LibraryDropTarget, StatusDefinition } from "@/lib/types";

export const gameDragId = (appId: number) => `game:${appId}`;
export const statusDropId = (statusId: string) => `status:${statusId}`;
export const collectionDropId = (collectionId: string) => `collection:${collectionId}`;
export const collectionPositionDropId = (collectionId: string, beforeAppId: number) =>
  `collection-position:${encodeURIComponent(collectionId)}:${beforeAppId}`;
export const manualPositionDropId = (beforeAppId: number) => `manual-position:${beforeAppId}`;
export const collectionOrderDragId = (collectionId: string) =>
  `collection-order:${encodeURIComponent(collectionId)}`;

export function draggedAppIds(activeAppId: number, selected: Set<number>): number[] {
  if (!selected.has(activeAppId)) return [activeAppId];
  return Array.from(selected);
}

export function parseGameDragId(value: string | number): number | undefined {
  const match = /^game:(\d+)$/.exec(String(value));
  if (!match) return undefined;
  const appId = Number(match[1]);
  return Number.isSafeInteger(appId) && appId > 0 ? appId : undefined;
}

export function parseCollectionOrderDragId(value: string | number): string | undefined {
  const match = /^collection-order:(.+)$/.exec(String(value));
  if (!match?.[1]) return undefined;
  try {
    const id = decodeURIComponent(match[1]);
    return id.trim() ? id : undefined;
  } catch {
    return undefined;
  }
}

export function reorderCollectionIds(ids: string[], activeId: string, overId: string): string[] {
  const from = ids.indexOf(activeId);
  const to = ids.indexOf(overId);
  if (from < 0 || to < 0 || from === to) return ids;
  const next = ids.slice();
  const [moved] = next.splice(from, 1);
  if (!moved) return ids;
  next.splice(to, 0, moved);
  return next;
}

export function parseLibraryDropTarget(
  value: string | number,
  statuses: StatusDefinition[],
  collections: CollectionSummary[],
): { target: LibraryDropTarget; label: string } | undefined {
  const id = String(value);
  if (id.startsWith("manual-position:")) {
    const beforeAppId = Number(id.slice("manual-position:".length));
    return Number.isSafeInteger(beforeAppId) && beforeAppId > 0
      ? { target: { kind: "manual", beforeAppId }, label: "orden manual de la biblioteca" }
      : undefined;
  }
  if (id.startsWith("collection-position:")) {
    const match = /^collection-position:(.+):(\d+)$/.exec(id);
    if (!match?.[1] || !match[2]) return undefined;
    let collectionId: string;
    try {
      collectionId = decodeURIComponent(match[1]);
    } catch {
      return undefined;
    }
    const beforeAppId = Number(match[2]);
    const collection = collections.find((item) => item.id === collectionId);
    if (collection?.kind !== "manual" || !Number.isSafeInteger(beforeAppId) || beforeAppId <= 0) {
      return undefined;
    }
    return {
      target: { kind: "collection", id: collection.id, beforeAppId },
      label: `posición en ${collection.name}`,
    };
  }
  if (id.startsWith("status:")) {
    const statusId = id.slice("status:".length);
    const status = statuses.find((item) => item.id === statusId);
    return status
      ? { target: { kind: "status", id: status.id }, label: `estado ${status.name}` }
      : undefined;
  }
  if (id.startsWith("collection:")) {
    const collectionId = id.slice("collection:".length);
    const collection = collections.find((item) => item.id === collectionId);
    if (collection?.kind !== "manual") return undefined;
    return {
      target: { kind: "collection", id: collection.id },
      label: `colección ${collection.name}`,
    };
  }
  return undefined;
}
