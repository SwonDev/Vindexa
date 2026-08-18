import { invoke } from "@tauri-apps/api/core";
import type { WishlistImportReport } from "@/lib/types";

/**
 * Contrato de la importación de deseados desde la sesión del navegador.
 *
 * Vive junto a la pantalla que lo consume, igual que `library-views.ts` guarda
 * el contrato de las vistas guardadas: quien lo mantiene es esta carpeta, y
 * `src/lib/tauri.ts` sólo tendría que reexportarlo si algún día hiciera falta
 * desde otra pantalla.
 *
 * # Por qué existe una segunda importación
 *
 * `import_steam_wishlist` pregunta a la API pública de Steam con el SteamID64.
 * Ese camino respeta la privacidad del perfil: con la cuenta en «Sólo amigos»
 * Steam devuelve una lista vacía y no hay forma de convencerlo desde fuera.
 *
 * Esta otra importación no pregunta desde fuera: abre el navegador integrado en
 * tu propia lista y lee lo que Steam ya te ha renderizado **a ti**. Por eso
 * funciona con el perfil cerrado, y por eso lo primero que puede pedirte es que
 * inicies sesión en esa ventana.
 */
export interface BrowserWishlistImportResult {
  report: WishlistImportReport;
  /** Cuenta de Steam cuya lista se ha leído, según la propia página. */
  steamId: string;
  /** Juegos cuyo nombre no pudo resolverse en la tienda. */
  titlesUnresolved: number;
  /**
   * Juegos que los filtros de la propia lista de Steam dejaron fuera.
   *
   * Es la diferencia entre el recuento que publica Steam y lo que la página
   * llegó a mostrar. Se enseña en vez de callarse: si tu lista tiene filtros
   * activos, la importación no puede traer lo que la página no enseña.
   */
  hiddenByFilters: number;
}

/**
 * Abre el navegador integrado en tu lista de deseados y la importa.
 *
 * Si no hay sesión iniciada, la ventana se queda abierta en la página de inicio
 * de sesión de Steam y la llamada falla con `wishlist_browser_signed_out`: se
 * inicia sesión ahí mismo y se vuelve a pulsar.
 */
export function importWishlistFromBrowser(): Promise<BrowserWishlistImportResult> {
  return invoke<BrowserWishlistImportResult>("import_steam_wishlist_from_browser");
}

/** Código de error que devuelve Vindexa, cuando el fallo trae uno. */
export function errorCode(error: unknown): string | undefined {
  if (error && typeof error === "object") {
    const candidate = error as { code?: unknown };
    if (typeof candidate.code === "string") return candidate.code;
  }
  return undefined;
}

/**
 * Códigos con los que el camino del navegador es la salida, no otro error.
 *
 * Son los dos casos en los que la API pública se queda sin nada que ofrecer
 * porque el perfil no es público. Verlos es lo que justifica ofrecer el otro
 * botón en ese preciso momento, en vez de tenerlo siempre a la vista pidiendo
 * que alguien adivine cuál de los dos toca.
 */
const PRIVATE_PROFILE_CODES = new Set(["steam_wishlist_private", "steam_not_linked"]);

export function suggestsBrowserImport(error: unknown): boolean {
  const code = errorCode(error);
  return code !== undefined && PRIVATE_PROFILE_CODES.has(code);
}
