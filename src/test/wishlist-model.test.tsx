import { describe, expect, it } from "vitest";
import {
  accentToken,
  applyBoardMove,
  formatCents,
  groupVideosByKind,
  moveWithin,
  normalizeBuckets,
  parsePriceInput,
  priceInputValue,
  summarizeTargets,
  WISHLIST_BUCKETS,
} from "@/features/wishlist/wishlist-model";
import type {
  GameSummary,
  GameVideo,
  WishlistBucket,
  WishlistBucketId,
  WishlistEntry,
  WishlistOverview,
} from "@/lib/types";

function game(appId: number, title: string): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: false,
    statusId: "backlog",
    statusName: "Pendiente",
    statusColor: "#5CAAC1",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 0,
    collectionIds: [],
  };
}

function entry(
  appId: number,
  title: string,
  bucket: WishlistBucketId,
  position: number,
  extra: Partial<WishlistEntry> = {},
): WishlistEntry {
  return {
    game: game(appId, title),
    bucket,
    priority: 0,
    position,
    note: "",
    addedAt: "2026-08-01T10:00:00Z",
    updatedAt: "2026-08-01T10:00:00Z",
    ...extra,
  };
}

function bucket(id: WishlistBucketId, items: WishlistEntry[]): WishlistBucket {
  return { bucket: id, items, total: items.length };
}

function overview(partial: Partial<WishlistOverview>): WishlistOverview {
  return {
    buckets: [],
    total: 0,
    targetTotals: [],
    entriesWithoutTarget: 0,
    ...partial,
  };
}

describe("agregado de precios objetivo", () => {
  it("sin ningún precio anotado no inventa una cifra", () => {
    const summary = summarizeTargets(overview({ total: 3, entriesWithoutTarget: 3 }));

    expect(summary.headline).toBe("Sin precio objetivo");
    expect(summary.atLeast).toBe(true);
    expect(summary.caveat).toContain("3 pendientes");
  });

  it("con todas las entradas valoradas presenta el total cerrado, sin «al menos»", () => {
    const summary = summarizeTargets(
      overview({
        total: 2,
        entriesWithoutTarget: 0,
        targetTotals: [{ currency: "EUR", totalCents: 4498, entries: 2 }],
      }),
    );

    expect(summary.atLeast).toBe(false);
    expect(summary.headline).not.toContain("Al menos");
    expect(summary.headline).toContain("44,98");
    expect(summary.caveat).toBe("");
  });

  it("con entradas sin precio el total se presenta como suelo, nunca como cierre", () => {
    const summary = summarizeTargets(
      overview({
        total: 5,
        entriesWithoutTarget: 2,
        targetTotals: [{ currency: "EUR", totalCents: 4498, entries: 3 }],
      }),
    );

    expect(summary.atLeast).toBe(true);
    expect(summary.headline.startsWith("Al menos ")).toBe(true);
    expect(summary.caveat).toContain("2 entradas sin precio objetivo");
    expect(summary.caveat).toContain("mínimo");
  });

  it("nunca suma monedas distintas: las enseña separadas y lo dice", () => {
    const summary = summarizeTargets(
      overview({
        total: 4,
        entriesWithoutTarget: 1,
        targetTotals: [
          { currency: "EUR", totalCents: 1000, entries: 2 },
          { currency: "USD", totalCents: 2000, entries: 1 },
        ],
      }),
    );

    expect(summary.currencies.map((item) => item.currency)).toEqual(["EUR", "USD"]);
    expect(summary.headline).toContain("10,00");
    expect(summary.headline).toContain("20,00");
    // 3000 céntimos es la suma prohibida: no puede aparecer en ninguna forma.
    expect(summary.headline).not.toContain("30,00");
    expect(summary.caveat).toContain("nunca sumadas entre sí");
  });

  it("respeta la moneda de cada total y no la traduce a la del sistema", () => {
    const summary = summarizeTargets(
      overview({
        targetTotals: [{ currency: "USD", totalCents: 2000, entries: 1 }],
      }),
    );

    expect(summary.currencies[0]?.amount).toMatch(/US\$|\$|USD/);
  });
});

describe("formato de dinero", () => {
  it("escribe la cifra en la moneda anotada", () => {
    expect(formatCents(1999, "EUR")).toContain("19,99");
    expect(formatCents(1999, "EUR")).toContain("€");
  });

  it("sin moneda deja la cifra desnuda en lugar de suponer euros", () => {
    expect(formatCents(1999)).toBe("19,99");
  });

  it("acepta lo que se teclea con coma y rechaza lo que no es un importe", () => {
    expect(parsePriceInput("19,99")).toBe(1999);
    expect(parsePriceInput("  7 ")).toBe(700);
    expect(parsePriceInput("")).toBeUndefined();
    expect(parsePriceInput("gratis")).toBeNull();
    expect(parsePriceInput("-3")).toBeNull();
    expect(parsePriceInput("19,999")).toBeNull();
    expect(priceInputValue(1999)).toBe("19,99");
    expect(priceInputValue(undefined)).toBe("");
  });
});

describe("tablero de deseados", () => {
  it("siempre devuelve los cuatro carriles en el mismo orden", () => {
    const normalized = normalizeBuckets(
      overview({ buckets: [bucket("watching", [entry(30, "Vigilado", "watching", 0)])] }),
    );

    expect(normalized.map((lane) => lane.bucket)).toEqual(WISHLIST_BUCKETS.map((meta) => meta.id));
    expect(normalized[3]?.items).toHaveLength(1);
    expect(normalized[0]?.items).toEqual([]);
  });

  it("traslada una entrada al carril destino delante del juego indicado", () => {
    const board = normalizeBuckets(
      overview({
        buckets: [
          bucket("buying_now", [entry(10, "Uno", "buying_now", 0)]),
          bucket("waiting_sale", [
            entry(20, "Dos", "waiting_sale", 0),
            entry(21, "Tres", "waiting_sale", 1),
          ]),
        ],
      }),
    );

    const next = applyBoardMove(board, 10, "waiting_sale", 21);

    expect(next[0]?.items).toEqual([]);
    expect(next[1]?.items.map((item) => item.game.appId)).toEqual([20, 10, 21]);
    expect(next[1]?.items[1]?.bucket).toBe("waiting_sale");
    expect(next[1]?.items.map((item) => item.position)).toEqual([0, 1, 2]);
    expect(next[1]?.total).toBe(3);
    // El tablero original no se toca: la instantánea sirve para revertir.
    expect(board[1]?.items).toHaveLength(2);
  });

  it("sin ancla coloca la entrada al final del carril destino", () => {
    const board = normalizeBuckets(
      overview({
        buckets: [
          bucket("buying_now", [entry(10, "Uno", "buying_now", 0)]),
          bucket("considering", [entry(30, "Tres", "considering", 0)]),
        ],
      }),
    );

    const next = applyBoardMove(board, 10, "considering");

    expect(next[2]?.items.map((item) => item.game.appId)).toEqual([30, 10]);
  });

  it("reordena dentro del carril sin perder ni duplicar entradas", () => {
    const ids = [1, 2, 3, 4];

    expect(moveWithin(ids, 0, 2)).toEqual([2, 3, 1, 4]);
    expect(moveWithin(ids, 3, 0)).toEqual([4, 1, 2, 3]);
    expect(moveWithin(ids, 0, 9)).toEqual(ids);
  });
});

describe("vídeos y listas", () => {
  const video = (videoId: string, kind: GameVideo["kind"], position: number): GameVideo => ({
    appId: 10,
    videoId,
    provider: "youtube",
    kind,
    title: videoId,
    channel: "Canal",
    source: "manual",
    position,
    createdAt: "2026-08-01T10:00:00Z",
    embedUrl: `https://www.youtube-nocookie.com/embed/${videoId}`,
  });

  it("agrupa por tipo en el orden del vocabulario y ordena por posición", () => {
    const groups = groupVideosByKind([
      video("b", "review", 1),
      video("a", "review", 0),
      video("c", "gameplay", 0),
    ]);

    expect(groups.map((group) => group.kind)).toEqual(["gameplay", "review"]);
    expect(groups[1]?.items.map((item) => item.videoId)).toEqual(["a", "b"]);
  });

  it("solo pinta acentos que existen como token del sistema", () => {
    expect(accentToken("cyan")).toBe("var(--v-cyan)");
    expect(accentToken("lime")).toBe("var(--v-lime)");
    // Un acento heredado que el sistema no tiene cae al neutro en vez de
    // inventar un color nuevo o reutilizar el rojo destructivo.
    expect(accentToken("rose")).toBe("var(--v-muted)");
    expect(accentToken("violet")).toBe("var(--v-muted)");
  });
});
