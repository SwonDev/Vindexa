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
 * Un solo listado para los catálogos que no son la biblioteca.
 *
 * Los juegos de las tiendas vinculadas no son propiedad de quien usa Vindexa,
 * pero tienen que navegarse igual: cambiar de sección no puede cambiar cómo se
 * mueve uno. Antes eran una lista de texto dentro de Ajustes, sin portadas.
 *
 * Los del préstamo familiar ya no pasan por aquí: viven en la biblioteca y usan
 * su mismo navegador, con estados, colecciones y ficha.
 *
 * Esta prueba vigila que el listado de catálogo siga siendo uno solo y que
 * conserve las tres piezas compartidas con la biblioteca.
 */
describe("los catálogos usan el mismo listado que la biblioteca", () => {
  const catalogo = readFileSync("src/features/library/CatalogBrowser.tsx", "utf8");
  const tienda = readFileSync("src/features/library/StoreCatalogBrowser.tsx", "utf8");

  it("calcula las columnas con la misma función que la biblioteca", () => {
    expect(catalogo).toContain("getGridColumns(");
    // La cuenta antigua no descontaba los huecos entre columnas.
    expect(catalogo).not.toMatch(/Math\.floor\(\(width - \d+\) \/ \d+\)/);
  });

  it("pinta el fundido de borde y lo alimenta al desplazar", () => {
    expect(catalogo).toContain("<LiquidEdge />");
    expect(catalogo).toContain("applyScrollEdgeFade(");
    // Sin esta marca el contenedor no recibe la altura del fundido.
    expect(catalogo).toContain('data-library-surface="true"');
  });

  it("resuelve las portadas por delante en lugar de al montar cada tarjeta", () => {
    expect(catalogo).toContain("prefetchArtwork(");
  });

  it("cada catálogo sólo traduce sus datos, no rehace el listado", () => {
    // Si un catálogo vuelve a montar su propia rejilla, esto lo detecta antes de
    // que las dos versiones empiecen a separarse.
    expect(tienda).toContain("<CatalogBrowser");
    expect(tienda).not.toContain("useVirtualizer");
  });
});
