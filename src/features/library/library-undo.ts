/**
 * Pila de deshacer de la biblioteca.
 *
 * Hasta ahora cada operación reversible guardaba su propio rastro suelto —un
 * recibo para los arrastres, una lista de orden para las colecciones— y no
 * había forma de deshacer una edición hecha con el teclado. La pila unifica las
 * tres: quien deshace no tiene que saber qué tipo de cambio hizo, sólo que fue
 * el último.
 *
 * Es deliberadamente **sólo deshacer**, sin rehacer: en una biblioteca, rehacer
 * después de tocar otra cosa restaura un estado que ya no encaja con lo que hay
 * en pantalla, y prometerlo sería peor que no ofrecerlo.
 */

import type { LibraryDropReceipt, UpdateGameInput } from "@/lib/types";

/**
 * Tope de la pila. Veinte cubre cualquier tanda de organización real y evita
 * quedarse con recibos de hace media hora que ya no describen la biblioteca.
 */
export const MAX_UNDO_ENTRIES = 20;

export type UndoEntry =
  /** Movimiento de uno o varios juegos a un estado o colección. */
  | { kind: "drop"; label: string; receipt: LibraryDropReceipt }
  /** Reordenación manual de la barra de colecciones. */
  | { kind: "collectionOrder"; label: string; previous: string[] }
  /** Edición de la ficha personal de un juego: estado, prioridad, fijado… */
  | { kind: "gameEdit"; label: string; previous: UpdateGameInput };

export function pushUndo(stack: readonly UndoEntry[], entry: UndoEntry): UndoEntry[] {
  return [...stack, entry].slice(-MAX_UNDO_ENTRIES);
}

export function peekUndo(stack: readonly UndoEntry[]): UndoEntry | undefined {
  return stack.at(-1);
}

export function popUndo(stack: readonly UndoEntry[]): {
  entry: UndoEntry | undefined;
  rest: UndoEntry[];
} {
  if (stack.length === 0) return { entry: undefined, rest: [] };
  return { entry: stack[stack.length - 1], rest: stack.slice(0, -1) };
}

/**
 * Retira de la pila las entradas que ya no se pueden aplicar.
 *
 * Un juego que desaparece de la biblioteca —porque se archivó o porque cambió
 * el ámbito— deja recibos que al deshacerse fallarían con un error que la
 * persona no puede entender. Es preferible que la acción no esté disponible a
 * que esté y no funcione.
 */
export function pruneUndo(
  stack: readonly UndoEntry[],
  knownAppIds: ReadonlySet<number>,
): UndoEntry[] {
  return stack.filter((entry) => {
    if (entry.kind === "gameEdit") return knownAppIds.has(entry.previous.appId);
    return true;
  });
}

/** Texto del botón y del anuncio para lectores de pantalla. */
export function describeUndo(entry: UndoEntry | undefined): string | undefined {
  if (!entry) return undefined;
  return `Deshacer: ${entry.label}`;
}
