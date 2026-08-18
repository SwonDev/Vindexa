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

/**
 * Detalles visuales que se rompen en silencio.
 *
 * Los tres nacieron de aritmética o de un valor por defecto del motor, no de
 * una decisión: nada falla, simplemente se ve mal, y sin una prueba vuelven al
 * primer descuido.
 */
describe("detalles visuales del sistema", () => {
  const css = readFileSync("src/index.css", "utf8");
  const conmutador = readFileSync("src/components/ui/switch.tsx", "utf8");
  const navegador = readFileSync("src/features/library/GameBrowser.tsx", "utf8");

  it("el campo de búsqueda no enseña dos aspas de borrar", () => {
    // WebKit dibuja la suya dentro de `input[type="search"]`, y junto a la del
    // proyecto salían dos.
    expect(css).toContain('input[type="search"]::-webkit-search-cancel-button');
    expect(css).toMatch(/-webkit-search-cancel-button[\s\S]{0,200}appearance: none/);
  });

  it("el pulgar del interruptor cabe en su carril", () => {
    // El carril lleva 1 px de borde y 2 px de relleno por lado, así que el hueco
    // mide seis menos: 34×20 deja 28×14, y 26×16 deja 20×10. Con el pulgar más
    // grande que el hueco, sobresalía y parecía descentrado.
    expect(conmutador).toContain("group-data-[size=default]/switch:size-3.5");
    expect(conmutador).toContain("group-data-[size=sm]/switch:size-2.5");
    // Y el recorrido es el hueco menos el pulgar.
    expect(conmutador).toContain("data-checked:translate-x-[14px]");
    expect(conmutador).toContain("data-checked:translate-x-[10px]");
  });

  it("el pulgar del interruptor no depende de una variante que no se aplica", () => {
    // Los tokens se declaran en `:root, .dark` pero nada añade la clase `dark`
    // al documento: la aplicación es oscura de raíz. El pulgar usaba
    // `dark:…bg-primary-foreground`, que nunca llegaba, y se quedaba en
    // `bg-background` —el fondo de la aplicación— sobre una pista azul.
    expect(conmutador).toContain("bg-foreground");
    expect(conmutador).not.toContain("dark:data-checked:");
    expect(conmutador).not.toContain("dark:data-unchecked:");
  });

  it("la tarjeta arrastrada no se traslada además del acompañante", () => {
    // El acompañante del cursor ya sigue al puntero. Trasladar también el
    // original movía el mismo elemento dos veces y, dentro de una fila
    // virtualizada que tiene su propia traslación, el resultado temblaba.
    expect(navegador).not.toContain("CSS.Translate");
    expect(navegador).not.toContain("@dnd-kit/utilities");
  });
});
