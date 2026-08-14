import { describe, expect, it } from "vitest";
import {
  collectionDropId,
  collectionOrderDragId,
  collectionPositionDropId,
  draggedAppIds,
  gameDragId,
  manualPositionDropId,
  parseCollectionOrderDragId,
  parseGameDragId,
  parseLibraryDropTarget,
  reorderCollectionIds,
  statusDropId,
} from "@/features/library/library-dnd";
import type { CollectionSummary, StatusDefinition } from "@/lib/types";

const statuses: StatusDefinition[] = [
  { id: "playing", name: "Jugando", color: "#66c0f4", position: 0, builtIn: true, gameCount: 2 },
];
const collections: CollectionSummary[] = [
  {
    id: "manual",
    name: "Cooperativos",
    description: "",
    color: "#66c0f4",
    icon: "folder",
    kind: "manual",
    matchMode: "all",
    position: 0,
    gameCount: 1,
  },
  {
    id: "smart",
    name: "Sin terminar",
    description: "",
    color: "#a4d007",
    icon: "sparkles",
    kind: "smart",
    matchMode: "all",
    position: 1,
    gameCount: 4,
  },
];

describe("contrato de arrastre de biblioteca", () => {
  it("arrastra toda la multiselección solo cuando el juego activo pertenece a ella", () => {
    expect(draggedAppIds(20, new Set([10, 20, 30]))).toEqual([10, 20, 30]);
    expect(draggedAppIds(40, new Set([10, 20, 30]))).toEqual([40]);
  });

  it("codifica y valida identificadores de juego y estado", () => {
    expect(parseGameDragId(gameDragId(730))).toBe(730);
    expect(parseGameDragId("game:0")).toBeUndefined();
    expect(parseLibraryDropTarget(statusDropId("playing"), statuses, collections)).toEqual({
      target: { kind: "status", id: "playing" },
      label: "estado Jugando",
    });
  });

  it("acepta colecciones manuales y rechaza colecciones inteligentes", () => {
    expect(parseLibraryDropTarget(collectionDropId("manual"), statuses, collections)).toEqual({
      target: { kind: "collection", id: "manual" },
      label: "colección Cooperativos",
    });
    expect(
      parseLibraryDropTarget(collectionDropId("smart"), statuses, collections),
    ).toBeUndefined();
  });

  it("convierte una carátula de colección manual en un ancla de inserción estable", () => {
    expect(
      parseLibraryDropTarget(collectionPositionDropId("manual", 730), statuses, collections),
    ).toEqual({
      target: { kind: "collection", id: "manual", beforeAppId: 730 },
      label: "posición en Cooperativos",
    });
  });

  it("convierte una carátula global en un ancla del orden manual", () => {
    expect(parseLibraryDropTarget(manualPositionDropId(730), statuses, collections)).toEqual({
      target: { kind: "manual", beforeAppId: 730 },
      label: "orden manual de la biblioteca",
    });
  });

  it("reordena colecciones por identificador sin perder ninguna", () => {
    const ids = ["uno", "dos", "tres"];
    expect(parseCollectionOrderDragId(collectionOrderDragId("dos"))).toBe("dos");
    expect(reorderCollectionIds(ids, "tres", "uno")).toEqual(["tres", "uno", "dos"]);
    expect(reorderCollectionIds(ids, "ausente", "uno")).toEqual(ids);
  });
});
