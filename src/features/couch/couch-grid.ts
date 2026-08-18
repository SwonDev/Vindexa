import type { GamepadDirection } from "@/hooks/use-gamepad";

/**
 * Geometría y recorrido de la rejilla del modo sofá.
 *
 * Vive aparte de la pantalla porque el número de columnas es consecuencia del
 * ancho medido y el recorrido tiene que poder comprobarse sin montar la vista
 * ni fingir un mando.
 */

/**
 * Ancho mínimo de una carátula. A dos metros de distancia una portada de 170 px
 * —la de la biblioteca de escritorio— es ilegible: aquí el mínimo es 240.
 */
export const COUCH_TILE_MIN_WIDTH = 240;
/** Hueco entre carátulas. Tiene que coincidir con el `gap` de `.couch__grid`. */
export const COUCH_GRID_GAP = 20;
export const COUCH_MIN_COLUMNS = 2;
export const COUCH_MAX_COLUMNS = 6;
/** Columnas mientras no hay ancho medido: coincide con el reparto habitual. */
export const COUCH_DEFAULT_COLUMNS = 4;

/**
 * Columnas que caben en un ancho dado, dentro de los límites del modo sofá.
 *
 * Los huecos entran en la cuenta: `n` columnas gastan `n - 1` huecos, y sin
 * descontarlos la última carátula se sale del contenedor y aparece cortada
 * contra la ficha.
 */
export function couchColumns(width: number): number {
  if (!Number.isFinite(width) || width <= 0) return COUCH_DEFAULT_COLUMNS;
  const fitting = Math.floor((width + COUCH_GRID_GAP) / (COUCH_TILE_MIN_WIDTH + COUCH_GRID_GAP));
  return Math.min(COUCH_MAX_COLUMNS, Math.max(COUCH_MIN_COLUMNS, fitting));
}

/** Índice válido dentro de la lista; con la lista vacía siempre es 0. */
export function clampCouchIndex(index: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(total - 1, Math.max(0, index));
}

/**
 * Desplaza el foco un número arbitrario de posiciones. Lo usan los gatillos
 * superiores, que saltan varias filas de una vez.
 */
export function stepCouchFocus(index: number, total: number, step: number): number {
  return clampCouchIndex(index + step, total);
}

/**
 * Mueve el foco una posición en la dirección pedida.
 *
 * Izquierda y derecha avanzan de uno en uno, así que al final de una fila el
 * foco continúa en la siguiente en lugar de chocar contra una pared invisible.
 * Arriba y abajo saltan una fila entera y se recortan a los extremos: bajar
 * desde la penúltima fila cuando la última está incompleta lleva al último
 * juego, no a ninguna parte. Es el mismo recorrido que ya usa la biblioteca de
 * escritorio, para que el modelo mental sea uno solo.
 */
export function moveCouchFocus(
  index: number,
  total: number,
  columns: number,
  direction: GamepadDirection,
): number {
  const rowWidth = Math.max(1, Math.floor(columns));
  const step =
    direction === "up"
      ? -rowWidth
      : direction === "down"
        ? rowWidth
        : direction === "left"
          ? -1
          : 1;
  return stepCouchFocus(index, total, step);
}

/** Filas que salta un gatillo superior. */
export const COUCH_PAGE_ROWS = 2;

/** Salto de página: dos filas completas arriba o abajo. */
export function pageCouchFocus(
  index: number,
  total: number,
  columns: number,
  direction: "up" | "down",
): number {
  const distance = Math.max(1, Math.floor(columns)) * COUCH_PAGE_ROWS;
  return stepCouchFocus(index, total, direction === "up" ? -distance : distance);
}
