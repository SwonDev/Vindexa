import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * Dónde tiene que haber clic derecho.
 *
 * # El fallo que la motivó
 *
 * Los menús contextuales existían sólo en la biblioteca. En Colecciones, en el
 * Planificador, en Deseados, en Seguimiento, en el modo salón y en las vistas
 * guardadas, el clic derecho no hacía nada: ni menú propio ni menú del sistema,
 * porque las guardas del cromado cancelan el nativo en toda la aplicación.
 *
 * Cancelar el menú nativo es una decisión de la aplicación entera; ofrecer uno
 * propio no puede quedarse en una pantalla.
 *
 * # Qué vigila
 *
 * Que cada pantalla que enseña cosas sobre las que actuar siga montando un menú
 * contextual. No comprueba qué hay dentro —eso lo hacen las pruebas de cada
 * pantalla, pulsando de verdad—; comprueba que no vuelva a desaparecer de un
 * sitio entero sin que nadie se entere.
 */

/** Archivos que deben montar al menos un menú contextual, y por qué. */
const OBLIGATORIOS: Record<string, string> = {
  "features/library/GameBrowser.tsx": "cada juego de la biblioteca",
  "features/library/LibrarySidebar.tsx": "estados, colecciones y tiendas",
  "features/library/SavedViewsBar.tsx": "cada vista guardada",
  "features/collections/CollectionsScreen.tsx": "cada colección y cada juego dentro",
  "features/planner/PlannerScreen.tsx": "cada tarjeta y cada columna del plan",
  "features/wishlist/WishlistBoard.tsx": "cada entrada de deseados",
  "features/wishlist/CuratedListsPanel.tsx": "cada lista curada y cada juego suyo",
  "features/discovery/DiscoveryScreen.tsx": "cada juego del radar de seguimiento",
  "features/discovery/UpcomingReleasesBlock.tsx": "cada lanzamiento previsto",
  "features/couch/CouchScreen.tsx": "cada carátula del modo salón",
  "features/notifications/NotificationsPopover.tsx": "cada aviso",
};

describe("el clic derecho llega a toda la aplicación", () => {
  it.each(Object.entries(OBLIGATORIOS))("%s → %s", (archivo) => {
    const fuente = readFileSync(`src/${archivo}`, "utf8");
    expect(fuente).toMatch(/<ContextMenuTrigger|ContextMenu\b/);
  });

  it("ninguna pantalla principal se queda fuera de la lista", () => {
    // Una pantalla nueva entra arriba con su motivo, o entra aquí la razón por
    // la que no lleva menú. Lo que no puede es pasar inadvertida.
    const pantallas = readdirSync("src/features", { withFileTypes: true })
      .filter((entrada) => entrada.isDirectory())
      .flatMap((carpeta) =>
        readdirSync(`src/features/${carpeta.name}`)
          .filter((archivo) => archivo.endsWith("Screen.tsx"))
          .map((archivo) => `features/${carpeta.name}/${archivo}`),
      );
    const sinMenu = pantallas.filter((pantalla) => !(pantalla in OBLIGATORIOS));
    // `LibraryScreen` delega en `GameBrowser` y en `LibrarySidebar`, que sí lo
    // llevan; `WishlistScreen` delega en `WishlistBoard` y `CuratedListsPanel`.
    expect(sinMenu.sort()).toEqual([
      "features/library/LibraryScreen.tsx",
      "features/wishlist/WishlistScreen.tsx",
    ]);
  });
});

/**
 * Dónde tiene que haber vista rápida.
 *
 * Una carátula dice cómo se llama un juego; no dice cómo se ve. Eso hace falta
 * en todas las listas donde se decide algo —la biblioteca, los deseados, las
 * ofertas, los lanzamientos, las colecciones, el plan y el radar—, no sólo en
 * la que se estaba mirando el día que se añadió.
 *
 * Igual que con el clic derecho: la decisión es de la aplicación entera, así
 * que la cobertura se enumera y se comprueba.
 */
const CON_VISTA_RAPIDA: Record<string, string> = {
  "features/library/GameBrowser.tsx": "cada juego de la biblioteca, en rejilla y en lista",
  "features/collections/CollectionsScreen.tsx": "cada juego dentro de una colección",
  "features/planner/PlannerScreen.tsx": "cada tarjeta del plan",
  "features/wishlist/WishlistList.tsx": "cada fila de deseados",
  "features/wishlist/WishlistBoard.tsx": "cada oferta de tu lista",
  "features/discovery/DiscoveryScreen.tsx": "cada juego del radar",
  "features/discovery/UpcomingReleasesBlock.tsx": "cada lanzamiento previsto",
  "features/discovery/StoreDealsBlock.tsx": "cada oferta de la tienda",
};

describe("la vista rápida llega a todas las listas donde se decide algo", () => {
  it.each(Object.entries(CON_VISTA_RAPIDA))("%s → %s", (archivo) => {
    expect(readFileSync(`src/${archivo}`, "utf8")).toContain("<GamePreviewCard");
  });
});

/**
 * Por dónde sale la vista rápida.
 *
 * A la derecha de una carátula hay sitio; al lado de una fila que ocupa todo el
 * ancho, no: se volteaba a la izquierda, se topaba con la barra lateral y salía
 * cortada por el borde de la ventana.
 *
 * Esto se comprueba leyendo el código y no montándolo porque jsdom no coloca
 * nada: no hay rectángulos, así que no hay colisión que observar. Lo que sí se
 * puede exigir es que cada lista pida el lado que le corresponde.
 */
describe("la vista rápida sale por donde cabe", () => {
  it("el componente respeta el lado que le piden y no tapa la barra superior", () => {
    const fuente = readFileSync("src/components/common/GamePreviewCard.tsx", "utf8");
    expect(fuente).toContain("side={side}");
    expect(fuente).toContain("top: 96");
  });

  it.each([
    "features/wishlist/WishlistList.tsx",
    "features/wishlist/WishlistBoard.tsx",
    "features/discovery/StoreDealsBlock.tsx",
    "features/discovery/DiscoveryScreen.tsx",
    "features/discovery/UpcomingReleasesBlock.tsx",
  ])("%s abre por abajo, porque sus filas ocupan todo el ancho", (archivo) => {
    expect(readFileSync(`src/${archivo}`, "utf8")).toContain('side="bottom"');
  });

  it("la lista de la biblioteca abre por abajo y la rejilla por el lado", () => {
    // El mismo archivo monta las dos: la tarjeta tiene hueco a su derecha y la
    // fila no.
    const fuente = readFileSync("src/features/library/GameBrowser.tsx", "utf8");
    expect(fuente).toContain('side="bottom"');
    expect(fuente.match(/<GamePreviewCard \{\.\.\.previewProps\(game\)\}/g)).toHaveLength(2);
  });
});
