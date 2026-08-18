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
 * El catálogo de Steam Family y el de las tiendas vinculadas enseñan juegos que
 * no son propiedad de quien usa Vindexa, pero tienen que navegarse igual que la
 * biblioteca: cambiar de sección no puede cambiar cómo se mueve uno. Cuando cada
 * uno traía su propia rejilla, el de Family calculaba las columnas con una
 * fórmula que la biblioteca ya había abandonado, no pintaba el fundido de borde
 * y pedía cada portada al montar su tarjeta.
 *
 * Esta prueba vigila que ese listado siga siendo uno solo y que conserve las
 * tres piezas compartidas.
 */
describe("los catálogos usan el mismo listado que la biblioteca", () => {
  const catalogo = readFileSync("src/features/library/CatalogBrowser.tsx", "utf8");
  const familia = readFileSync("src/features/library/FamilyCatalogBrowser.tsx", "utf8");

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
    expect(familia).toContain("<CatalogBrowser");
    expect(familia).not.toContain("useVirtualizer");
  });
});
