import { describe, expect, it } from "vitest";
import {
  COUCH_DEFAULT_COLUMNS,
  COUCH_GRID_GAP,
  COUCH_MAX_COLUMNS,
  COUCH_MIN_COLUMNS,
  COUCH_TILE_MIN_WIDTH,
  clampCouchIndex,
  couchColumns,
  moveCouchFocus,
  pageCouchFocus,
  stepCouchFocus,
} from "@/features/couch/couch-grid";

describe("columnas del modo sofá", () => {
  it("reparte el ancho en carátulas que se lean a dos metros", () => {
    // En 1.000 px caben tres de 320: cuatro dejarían 235 y no llegan al mínimo.
    expect(couchColumns(1_000)).toBe(3);
    expect(couchColumns(1_920)).toBe(COUCH_MAX_COLUMNS);
    expect(couchColumns(400)).toBe(COUCH_MIN_COLUMNS);
  });

  it("las carátulas que resultan nunca bajan del ancho mínimo", () => {
    // La invariante que se rompía: sin descontar los huecos, la última columna
    // se salía del contenedor y aparecía cortada contra la ficha.
    for (let ancho = 620; ancho <= 2_400; ancho += 20) {
      const columnas = couchColumns(ancho);
      if (columnas === COUCH_MIN_COLUMNS || columnas === COUCH_MAX_COLUMNS) continue;
      const porCaratula = (ancho - (columnas - 1) * COUCH_GRID_GAP) / columnas;
      expect(porCaratula, `ancho ${ancho} con ${columnas} columnas`).toBeGreaterThanOrEqual(
        COUCH_TILE_MIN_WIDTH,
      );
    }
  });

  it("mientras no hay ancho medido usa el reparto habitual", () => {
    // Sin medida —primer fotograma, o un entorno sin `ResizeObserver`— no puede
    // caer a dos columnas: la rejilla saltaría en cuanto llegase la medida.
    expect(couchColumns(0)).toBe(COUCH_DEFAULT_COLUMNS);
    expect(couchColumns(Number.NaN)).toBe(COUCH_DEFAULT_COLUMNS);
    expect(couchColumns(-100)).toBe(COUCH_DEFAULT_COLUMNS);
  });
});

describe("recorrido de la rejilla", () => {
  const total = 10;
  const columns = 4;

  it("izquierda y derecha avanzan de uno en uno y cruzan de fila", () => {
    expect(moveCouchFocus(0, total, columns, "right")).toBe(1);
    expect(moveCouchFocus(1, total, columns, "left")).toBe(0);
    // Final de fila: el foco sigue en la primera columna de la siguiente.
    expect(moveCouchFocus(3, total, columns, "right")).toBe(4);
    expect(moveCouchFocus(4, total, columns, "left")).toBe(3);
  });

  it("arriba y abajo saltan una fila entera", () => {
    expect(moveCouchFocus(0, total, columns, "down")).toBe(4);
    expect(moveCouchFocus(4, total, columns, "down")).toBe(8);
    expect(moveCouchFocus(8, total, columns, "up")).toBe(4);
    expect(moveCouchFocus(5, total, columns, "up")).toBe(1);
  });

  it("no se sale de la rejilla por ningún extremo", () => {
    expect(moveCouchFocus(0, total, columns, "left")).toBe(0);
    expect(moveCouchFocus(0, total, columns, "up")).toBe(0);
    expect(moveCouchFocus(9, total, columns, "right")).toBe(9);
    // Última fila incompleta: bajar lleva al último juego, no a la nada.
    expect(moveCouchFocus(7, total, columns, "down")).toBe(9);
    expect(moveCouchFocus(9, total, columns, "down")).toBe(9);
  });

  it("cambia de forma cuando cambia el número de columnas", () => {
    expect(moveCouchFocus(0, total, 2, "down")).toBe(2);
    expect(moveCouchFocus(0, total, 6, "down")).toBe(6);
    // Un ancho absurdo no puede dejar el foco clavado ni moverlo hacia atrás.
    expect(moveCouchFocus(0, total, 0, "down")).toBe(1);
  });

  it("sobre una rejilla vacía el foco se queda en cero", () => {
    expect(moveCouchFocus(0, 0, columns, "down")).toBe(0);
    expect(clampCouchIndex(5, 0)).toBe(0);
    expect(clampCouchIndex(5, 3)).toBe(2);
    expect(clampCouchIndex(-2, 3)).toBe(0);
  });

  it("los gatillos superiores saltan dos filas completas", () => {
    expect(pageCouchFocus(0, 40, columns, "down")).toBe(8);
    expect(pageCouchFocus(20, 40, columns, "up")).toBe(12);
    expect(pageCouchFocus(2, 40, columns, "up")).toBe(0);
    expect(pageCouchFocus(38, 40, columns, "down")).toBe(39);
  });

  it("el salto arbitrario comparte los mismos límites", () => {
    expect(stepCouchFocus(3, 10, 4)).toBe(7);
    expect(stepCouchFocus(3, 10, 400)).toBe(9);
    expect(stepCouchFocus(3, 10, -400)).toBe(0);
  });
});
