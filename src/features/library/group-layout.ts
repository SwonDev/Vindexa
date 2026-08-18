import { groupIndex, type LibraryGroup } from "@/features/library/library-grouping";
import type { GameSummary } from "@/lib/types";

/** Alto en píxeles del encabezado de grupo, tanto en el lienzo como fijo. */
export const GROUP_HEADER_HEIGHT = 30;

export type LibraryRow =
  | { kind: "header"; key: string; label: string; loaded: number }
  | { kind: "games"; key: string; games: GameSummary[] };

/** Destino del índice de salto: etiqueta corta y fila a la que desplazarse. */
export interface GroupJump {
  key: string;
  label: string;
  row: number;
}

export interface LibraryLayout {
  rows: LibraryRow[];
  jumps: GroupJump[];
  /** Grupo al que pertenece cada fila; `-1` cuando no hay agrupación. */
  groupOfRow: number[];
  /** Fila de cada juego, para llevar el foco sin recorrer toda la lista. */
  rowOfGame: Map<number, number>;
}

/**
 * Convierte juegos y grupos en las filas que virtualiza la vista.
 *
 * `columns` es lo que permite que rejilla y lista compartan una sola forma de
 * cortar: con una columna sale la lista, con varias sale la rejilla y cada
 * grupo arranca en la columna cero porque su encabezado ocupa una fila entera.
 */
export function buildLibraryLayout(
  games: readonly GameSummary[],
  groups: readonly LibraryGroup[],
  columns: number,
): LibraryLayout {
  const width = Math.max(1, Math.floor(columns));
  const rows: LibraryRow[] = [];
  const groupOfRow: number[] = [];
  const rowOfGame = new Map<number, number>();
  const jumps: GroupJump[] = [];

  const pushGames = (bucket: readonly GameSummary[], group: number) => {
    for (let start = 0; start < bucket.length; start += width) {
      const slice = bucket.slice(start, start + width);
      const first = slice[0];
      if (!first) continue;
      for (const game of slice) rowOfGame.set(game.appId, rows.length);
      rows.push({ kind: "games", key: `fila:${first.appId}`, games: slice });
      groupOfRow.push(group);
    }
  };

  if (!groups.length) {
    pushGames(games, -1);
    return { rows, jumps, groupOfRow, rowOfGame };
  }

  const labels = groupIndex(groups);
  groups.forEach((group, index) => {
    jumps.push({ key: group.key, label: labels[index]?.label ?? group.key, row: rows.length });
    rows.push({
      kind: "header",
      key: group.key,
      label: group.label,
      loaded: group.games.length,
    });
    groupOfRow.push(index);
    pushGames(group.games, index);
  });
  return { rows, jumps, groupOfRow, rowOfGame };
}

/**
 * Desplazamiento acumulado de cada fila, con el total en la última posición.
 *
 * El virtualizador ya conoce estas medidas, pero solo de las filas montadas; el
 * encabezado fijo y el índice de salto necesitan las de cualquier fila, incluidas
 * las que están a mil pantallas de distancia.
 */
export function rowOffsets(rows: readonly LibraryRow[], gameRowHeight: number): number[] {
  const offsets = new Array<number>(rows.length + 1);
  let running = 0;
  for (let index = 0; index < rows.length; index += 1) {
    offsets[index] = running;
    running += rows[index]?.kind === "header" ? GROUP_HEADER_HEIGHT : gameRowHeight;
  }
  offsets[rows.length] = running;
  return offsets;
}

export interface StickyGroup {
  /** Posición dentro de `jumps`. */
  index: number;
  /** Cuánto empuja hacia arriba el encabezado siguiente; nunca positivo. */
  shift: number;
}

/**
 * Grupo cuyo encabezado queda tapado por la franja fija, y cuánto lo desplaza
 * el grupo siguiente al alcanzarla.
 *
 * `threshold` es el desplazamiento del lienzo que queda justo bajo la franja:
 * el llamante lo calcula porque cada vista arranca su lienzo a una altura
 * distinta dentro del contenedor desplazable.
 */
export function stickyGroupAt(
  jumps: readonly GroupJump[],
  offsets: readonly number[],
  threshold: number,
): StickyGroup | undefined {
  const index = passedGroupAt(jumps, offsets, threshold);
  if (index < 0) return undefined;
  const next = jumps[index + 1];
  if (!next) return { index, shift: 0 };
  const nextStart = offsets[next.row] ?? 0;
  return { index, shift: Math.min(0, nextStart - threshold - GROUP_HEADER_HEIGHT) };
}

/** Grupo por el que va la lectura ahora mismo, para marcarlo en el índice. */
export function currentGroupAt(
  jumps: readonly GroupJump[],
  offsets: readonly number[],
  threshold: number,
): number {
  if (!jumps.length) return -1;
  return Math.max(0, passedGroupAt(jumps, offsets, threshold));
}

/** Último encabezado que ya quedó por encima de `threshold`, o `-1`. */
function passedGroupAt(
  jumps: readonly GroupJump[],
  offsets: readonly number[],
  threshold: number,
): number {
  // Los desplazamientos crecen con el índice, así que basta con partir en dos:
  // agrupar por estudio deja cientos de encabezados y esto corre en cada scroll.
  let low = 0;
  let high = jumps.length - 1;
  let passed = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    const jump = jumps[middle];
    if (jump && (offsets[jump.row] ?? 0) <= threshold) {
      passed = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return passed;
}
