import { describe, expect, it } from "vitest";
import {
  applySelectionGesture,
  type SelectionState,
  selectionGestureFrom,
} from "@/features/library/library-selection";
import type { GameSummary } from "@/lib/types";

function game(appId: number): GameSummary {
  return {
    appId,
    title: `Juego ${appId}`,
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: false,
    statusId: "unclassified",
    statusName: "Sin clasificar",
    statusColor: "#8493A4",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: appId,
    drmState: "unknown",
    genres: [],
    collectionIds: [],
  };
}

const GAMES = [10, 20, 30, 40, 50].map(game);
const empty: SelectionState = { selected: new Set() };
const ids = (state: SelectionState) => [...state.selected].sort((a, b) => a - b);

describe("gestos de selección", () => {
  it("traduce cada combinación de modificadores", () => {
    expect(selectionGestureFrom({ metaKey: false, ctrlKey: false, shiftKey: false })).toBe(
      "replace",
    );
    expect(selectionGestureFrom({ metaKey: true, ctrlKey: false, shiftKey: false })).toBe("toggle");
    expect(selectionGestureFrom({ metaKey: false, ctrlKey: true, shiftKey: false })).toBe("toggle");
    expect(selectionGestureFrom({ metaKey: false, ctrlKey: false, shiftKey: true })).toBe("range");
    expect(selectionGestureFrom({ metaKey: true, ctrlKey: false, shiftKey: true })).toBe("range");
  });
});

describe("selección de biblioteca", () => {
  it("el clic simple reemplaza la selección y fija el ancla", () => {
    const state = applySelectionGesture(GAMES, empty, 30, "replace");
    expect(ids(state)).toEqual([30]);
    expect(state.anchor).toBe(30);
  });

  it("alternar añade y quita sin tocar el resto", () => {
    let state = applySelectionGesture(GAMES, empty, 10, "replace");
    state = applySelectionGesture(GAMES, state, 30, "toggle");
    expect(ids(state)).toEqual([10, 30]);
    state = applySelectionGesture(GAMES, state, 10, "toggle");
    expect(ids(state)).toEqual([30]);
  });

  it("extiende el rango hacia delante y hacia atrás desde el ancla", () => {
    const anchored = applySelectionGesture(GAMES, empty, 20, "replace");
    expect(ids(applySelectionGesture(GAMES, anchored, 40, "range"))).toEqual([20, 30, 40]);
    expect(ids(applySelectionGesture(GAMES, anchored, 10, "range"))).toEqual([10, 20]);
  });

  it("conserva el ancla al encadenar varios rangos", () => {
    const anchored = applySelectionGesture(GAMES, empty, 20, "replace");
    const wide = applySelectionGesture(GAMES, anchored, 50, "range");
    expect(wide.anchor).toBe(20);
    // Reducir el rango debe soltar lo que ya no cabe, no acumularlo.
    expect(ids(applySelectionGesture(GAMES, wide, 30, "range"))).toEqual([20, 30]);
  });

  it("degrada a selección simple cuando el ancla ya no está a la vista", () => {
    const stale: SelectionState = { selected: new Set([999]), anchor: 999 };
    const state = applySelectionGesture(GAMES, stale, 40, "range");
    expect(ids(state)).toEqual([40]);
    expect(state.anchor).toBe(40);
  });

  it("sin ancla previa, extender equivale a seleccionar", () => {
    expect(ids(applySelectionGesture(GAMES, empty, 30, "range"))).toEqual([30]);
  });

  it("extender sobre el propio ancla deja solo ese juego", () => {
    const anchored = applySelectionGesture(GAMES, empty, 30, "replace");
    expect(ids(applySelectionGesture(GAMES, anchored, 30, "range"))).toEqual([30]);
  });

  it("no se rompe con una biblioteca vacía", () => {
    expect(ids(applySelectionGesture([], empty, 10, "range"))).toEqual([10]);
  });
});
