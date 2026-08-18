import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import dndCss from "@/features/library/library-dnd.css?raw";

describe("espacio reservado para la selección flotante", () => {
  it("permite desplazar la última fila completamente por encima de la barra", () => {
    expect(dndCss).toContain(".library-main:has(.selection-bar) .game-browser");
    expect(dndCss).toMatch(/padding-bottom:\s*72px/);
    expect(dndCss).toMatch(/scroll-padding-bottom:\s*72px/);
  });
});

/**
 * Paridad visual entre la biblioteca y el catálogo de Steam Family.
 *
 * El catálogo tiene su propio componente porque su tarjeta dice otras cosas
 * —de quién viene el juego, si Steam lo ha confirmado—, pero el listado en sí
 * tiene que comportarse igual: mismas columnas, mismo fundido al desplazar y
 * las portadas resueltas por delante. Cuando cada uno traía su propia
 * aritmética, a igual ancho salían tarjetas de otro tamaño.
 */
describe("el catálogo de Family se comporta como la biblioteca", () => {
  const familia = readFileSync("src/features/library/FamilyCatalogBrowser.tsx", "utf8");

  it("calcula las columnas con la misma función que la biblioteca", () => {
    expect(familia).toContain("getGridColumns(");
    // La cuenta antigua no descontaba los huecos entre columnas.
    expect(familia).not.toMatch(/Math\.floor\(\(width - \d+\) \/ \d+\)/);
  });

  it("pinta el fundido de borde y lo alimenta al desplazar", () => {
    expect(familia).toContain("<LiquidEdge />");
    expect(familia).toContain("applyScrollEdgeFade(");
    // Sin esta marca el contenedor no recibe la altura del fundido.
    expect(familia).toContain('data-library-surface="true"');
  });

  it("resuelve las portadas por delante en lugar de al montar cada tarjeta", () => {
    expect(familia).toContain("prefetchArtwork(");
  });
});
