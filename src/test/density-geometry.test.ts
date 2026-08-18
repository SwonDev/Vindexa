import { describe, expect, it } from "vitest";
import { listRowBoxHeight, listRowHeight } from "@/features/library/density-list-rows";
import {
  DENSITY_METRICS,
  GRID_TILE_MIN_WIDTH,
  getGridColumns,
  getVirtualGridGeometry,
} from "@/features/shell/interface-density";

/**
 * La garantía que importa no es una altura concreta —cambia con cada ajuste de
 * ritmo— sino que las filas queden contiguas: sin solape ni hueco, para que la
 * lista virtualizada no deje bandas muertas ni recorte tarjetas.
 */
describe("geometría virtual por densidad", () => {
  it.each(["compact", "comfortable"] as const)("mantiene filas contiguas en modo %s", (density) => {
    const width = 900;
    const columns = 5;
    const geometry = getVirtualGridGeometry(width, columns, 11, density);
    const metrics = DENSITY_METRICS[density];
    const cardWidth = (width - metrics.gridPadding - (columns - 1) * metrics.gridGap) / columns;

    expect(geometry.rowCount).toBe(3);
    // La fila reserva la portada 2:3 completa más el cuerpo de texto.
    expect(geometry.rowHeight).toBe(Math.ceil(cardWidth * 1.5 + metrics.gridBody));
    expect([0, 1, 2].map(geometry.rowStart)).toEqual([
      0,
      geometry.rowHeight,
      geometry.rowHeight * 2,
    ]);
    expect(geometry.totalHeight).toBe(geometry.rowHeight * 3);
    expect(geometry.rowStart(2) + geometry.rowHeight).toBe(geometry.totalHeight);
  });

  it("el modo cómodo respira más que el compacto", () => {
    expect(DENSITY_METRICS.comfortable.gridGap).toBeGreaterThan(DENSITY_METRICS.compact.gridGap);
    expect(DENSITY_METRICS.comfortable.gridBody).toBeGreaterThan(DENSITY_METRICS.compact.gridBody);
  });

  it("el ritmo de la retícula descansa sobre la unidad base de 4 px", () => {
    for (const metrics of Object.values(DENSITY_METRICS)) {
      expect(metrics.gridGap % 4).toBe(0);
      expect(metrics.gridPadding % 4).toBe(0);
      expect(metrics.gridBody % 4).toBe(0);
    }
  });
});

describe("geometría de las filas tabulares", () => {
  it.each(["compact", "comfortable"] as const)(
    "deja el recuadro justo por debajo del paso en modo %s",
    (density) => {
      for (const ultraCompact of [false, true]) {
        const step = listRowHeight(density, ultraCompact);
        const box = listRowBoxHeight(step);
        // Contiguas y sin solape: el recuadro nunca invade el paso siguiente.
        expect(box).toBeLessThan(step);
        expect(step - box).toBe(1);
        expect(step % 2).toBe(0);
      }
    },
  );

  it("la ultracompacta es siempre más apretada que la lista", () => {
    for (const density of ["compact", "comfortable"] as const) {
      expect(listRowHeight(density, true)).toBeLessThan(listRowHeight(density, false));
    }
    expect(listRowHeight("comfortable", true)).toBeGreaterThan(listRowHeight("compact", true));
  });

  it("la ultracompacta cumple lo que promete a 1440×900", () => {
    // Alto que el shell deja al listado a 1440×900, ya descontada la cabecera
    // de columnas: si ahí no caben veintiocho filas, la vista no es tal.
    const availableHeight = 728;
    expect(Math.floor(availableHeight / listRowHeight("compact", true))).toBeGreaterThanOrEqual(28);
  });
});

describe("columnas de la rejilla", () => {
  it("nunca deja una carátula por debajo del ancho mínimo", () => {
    // La invariante que se rompía: ensanchar la ventana metía una columna de
    // más y encogía **todas** las carátulas, así que agrandar la ventana las
    // hacía más pequeñas. Medido antes del arreglo: 1.130 px daba 171 px por
    // carátula y 1.610 px daba 162.
    for (const density of ["compact", "comfortable"] as const) {
      const metrics = DENSITY_METRICS[density];
      for (let ancho = 620; ancho <= 2600; ancho += 10) {
        const columnas = getGridColumns(ancho, density);
        if (columnas <= 2) continue;
        const porCaratula =
          (ancho - metrics.gridPadding - (columnas - 1) * metrics.gridGap) / columnas;
        expect(
          porCaratula,
          `${density} a ${ancho} px reparte ${columnas} columnas de ${porCaratula.toFixed(1)} px`,
        ).toBeGreaterThanOrEqual(GRID_TILE_MIN_WIDTH);
      }
    }
  });

  it("aprovecha el ancho: más ventana nunca da menos columnas", () => {
    let previas = 0;
    for (let ancho = 620; ancho <= 2600; ancho += 10) {
      const columnas = getGridColumns(ancho, "compact");
      expect(columnas, `retrocede en ${ancho} px`).toBeGreaterThanOrEqual(previas);
      previas = columnas;
    }
  });
});
