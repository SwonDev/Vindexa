import type { GameSummary } from "@/lib/types";

/**
 * Gestos de selección de la biblioteca.
 *
 * `replace` es el clic simple, `toggle` el clic con Cmd/Ctrl y `range` el clic
 * con Mayús, que extiende desde el último juego tocado sin modificadores. Son
 * los tres gestos que cualquier gestor de archivos de escritorio ofrece desde
 * hace décadas y que quien tiene mil juegos espera encontrar aquí.
 */
export type SelectionGesture = "replace" | "toggle" | "range";

/** Traduce el estado de los modificadores del ratón al gesto correspondiente. */
export function selectionGestureFrom(event: {
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
}): SelectionGesture {
  // Mayús manda: extender un rango nunca debe degradar a alternar.
  if (event.shiftKey) return "range";
  return event.metaKey || event.ctrlKey ? "toggle" : "replace";
}

export interface SelectionState {
  selected: ReadonlySet<number>;
  /** Último juego tocado sin Mayús; es el extremo fijo del rango. */
  anchor?: number | undefined;
}

/**
 * Calcula la selección resultante de un gesto.
 *
 * Es una función pura sobre la página cargada: si el ancla ya no está a la
 * vista —porque cambió el filtro o el orden—, el rango degrada a una selección
 * simple en lugar de seleccionar algo que la persona no ve.
 */
export function applySelectionGesture(
  games: readonly GameSummary[],
  state: SelectionState,
  appId: number,
  gesture: SelectionGesture,
): SelectionState {
  if (gesture === "range" && state.anchor !== undefined && state.anchor !== appId) {
    const from = games.findIndex((game) => game.appId === state.anchor);
    const to = games.findIndex((game) => game.appId === appId);
    if (from >= 0 && to >= 0) {
      const [start, end] = from <= to ? [from, to] : [to, from];
      return {
        selected: new Set(games.slice(start, end + 1).map((game) => game.appId)),
        // El ancla no se mueve: encadenar varios Mayús extiende desde el mismo
        // extremo, como en Finder o en el Explorador.
        anchor: state.anchor,
      };
    }
  }

  if (gesture === "toggle") {
    const next = new Set(state.selected);
    if (next.has(appId)) next.delete(appId);
    else next.add(appId);
    return { selected: next, anchor: appId };
  }

  return { selected: new Set([appId]), anchor: appId };
}
