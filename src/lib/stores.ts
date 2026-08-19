import type { ExternalStoreId } from "@/lib/types";

/**
 * Cómo se llama cada tienda cuando se enseña.
 *
 * Vive aquí y no en cada pantalla porque el nombre tiene que ser el mismo en la
 * barra lateral, en la ficha de un juego, en la lista y en Ajustes: cuando cada
 * sitio ponía el suyo, la misma tienda era «Epic Games» en un lado y «Epic
 * Games Store» en otro, y itch.io no aparecía en ninguno.
 */
const STORE_LABELS: Record<string, string> = {
  epic: "Epic Games Store",
  gog: "GOG",
  itch: "itch.io",
};

/**
 * Nombre corto, para donde el ancho manda: la barra lateral y las filas de la
 * lista. Es el mismo nombre recortado, no otro distinto.
 */
const STORE_SHORT_LABELS: Record<string, string> = {
  epic: "Epic Games",
  gog: "GOG",
  itch: "itch.io",
};

/** Nombre propio de la tienda. Se usa donde hay sitio para escribirlo entero. */
export function storeLabel(store: ExternalStoreId | string): string {
  return STORE_LABELS[store] ?? store;
}

export function storeShortLabel(store: ExternalStoreId | string): string {
  return STORE_SHORT_LABELS[store] ?? storeLabel(store);
}

/** ¿Este identificador lo asignó Vindexa en vez de Steam? */
export const LOCAL_APP_ID_BASE = 2_000_000_000;

/**
 * Un juego que no es de Steam no tiene AppID.
 *
 * Vindexa le da uno local a partir de {@link LOCAL_APP_ID_BASE} para poder
 * organizarlo como a los demás, pero ese número **no significa nada en Steam**:
 * no se enseña como si lo fuera ni se manda a ninguna de sus APIs.
 */
export function isLocalAppId(appId: number): boolean {
  return appId >= LOCAL_APP_ID_BASE;
}
