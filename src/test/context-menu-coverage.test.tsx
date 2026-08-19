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
