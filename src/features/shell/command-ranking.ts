import type { GameSummary } from "@/lib/types";

/**
 * Puntuación de la paleta de comandos.
 *
 * Vive fuera del componente porque es la pieza que decide qué se ve al escribir
 * y necesita comprobarse con títulos reales, sin montar un diálogo. La regla que
 * gobierna todo el módulo: una coincidencia sólo vale si se puede señalar en el
 * título. Una subsecuencia de letras repartidas por el nombre —«elden» dentro de
 * «Marvel's Spider-Man Remastered»— no es un resultado, es ruido.
 */

/** Resultados de juego de la página cargada. Ocho caben sin desplazar. */
export const GAME_RESULT_LIMIT = 8;

/** Puntuación mínima para considerar que hay coincidencia. */
export const MATCH_THRESHOLD = 0;

const DIACRITICS = /\p{Diacritic}/gu;

/** Minúsculas sin tildes: «Ōkami» y «okami» deben encontrarse igual. */
export function normalizeSearchText(value: string): string {
  return value.normalize("NFD").replace(DIACRITICS, "").toLowerCase();
}

/** Bandas de puntuación. La distancia entre ellas hace el orden predecible. */
const PREFIX_BASE = 4000;
const WORD_START_BASE = 3000;
const INNER_BASE = 2000;
const SUBSEQUENCE_BASE = 1000;

/** Letra que arranca palabra: es lo que se teclea al abreviar («p4g»). */
const WORD_START_BONUS = 40;
/** Letra pegada a la anterior: fragmento reconocible dentro del título. */
const CONTIGUOUS_BONUS = 24;
/**
 * Letra suelta —ni inicio de palabra ni contigua—. Una se tolera, porque suele
 * ser una errata; dos hunden la puntuación bajo el umbral y descartan la fila.
 */
const LOOSE_PENALTY = 600;

/**
 * Puntuación difusa. Devuelve `-1` cuando no hay coincidencia.
 *
 * Premia, por este orden: el prefijo exacto, el fragmento exacto tras un
 * espacio, el fragmento exacto en cualquier posición y, por último, la
 * subsecuencia, que sólo sobrevive si casi todas sus letras caen en un inicio de
 * palabra o pegadas a la anterior. Todo con `indexOf`, que es código nativo, y
 * sin reservar memoria.
 */
export function fuzzyScore(haystack: string, needle: string): number {
  if (!needle) return 0;
  if (needle.length > haystack.length) return -1;
  const direct = haystack.indexOf(needle);
  if (direct === 0) return PREFIX_BASE - haystack.length;
  if (direct > 0) {
    const boundary = haystack.charCodeAt(direct - 1) === 32;
    return (boundary ? WORD_START_BASE : INNER_BASE) - direct - haystack.length;
  }
  let starts = 0;
  let contiguous = 0;
  let loose = 0;
  let cursor = 0;
  let previous = -2;
  for (let index = 0; index < needle.length; index += 1) {
    const found = haystack.indexOf(needle.charAt(index), cursor);
    if (found < 0) return -1;
    if (found === 0 || haystack.charCodeAt(found - 1) === 32) starts += 1;
    else if (found === previous + 1) contiguous += 1;
    else loose += 1;
    previous = found;
    cursor = found + 1;
  }
  const score =
    SUBSEQUENCE_BASE +
    starts * WORD_START_BONUS +
    contiguous * CONTIGUOUS_BONUS -
    loose * LOOSE_PENALTY -
    haystack.length / 8;
  return score < MATCH_THRESHOLD ? -1 : score;
}

/**
 * Caché del título normalizado. Es un `WeakMap` a propósito: la clave es el
 * propio objeto del juego, que React Query conserva entre renderizados y
 * sustituye al refrescar, de modo que la caché se invalida sola y no crece.
 */
const normalizedTitles = new WeakMap<GameSummary, string>();

export function gameHaystack(game: GameSummary): string {
  const cached = normalizedTitles.get(game);
  if (cached !== undefined) return cached;
  const value = normalizeSearchText(game.title);
  normalizedTitles.set(game, value);
  return value;
}

/**
 * Mejores `limit` juegos para la consulta, en una sola pasada.
 *
 * No se ordena el catálogo: se mantiene una lista de tamaño `limit` por
 * inserción, así que el coste es O(n·limit) con `limit = 8` y sin reservar un
 * array intermedio por pulsación.
 */
export function rankGames(
  games: readonly GameSummary[],
  query: string,
  limit = GAME_RESULT_LIMIT,
): GameSummary[] {
  if (limit <= 0) return [];
  const needle = normalizeSearchText(query.trim());
  if (!needle) return games.slice(0, limit);
  const best: GameSummary[] = [];
  const scores: number[] = [];
  for (const game of games) {
    const score = fuzzyScore(gameHaystack(game), needle);
    if (score < 0) continue;
    if (best.length === limit && score <= (scores[limit - 1] ?? 0)) continue;
    let position = best.length < limit ? best.length : limit - 1;
    while (position > 0 && (scores[position - 1] ?? 0) < score) {
      best[position] = best[position - 1] as GameSummary;
      scores[position] = scores[position - 1] as number;
      position -= 1;
    }
    best[position] = game;
    scores[position] = score;
    if (best.length > limit) {
      best.length = limit;
      scores.length = limit;
    }
  }
  return best;
}

export interface PaletteGameResult {
  game: GameSummary;
  /** No estaba en la página cargada: lo trajo la consulta a SQLite. */
  fromCatalog: boolean;
}

/**
 * Une lo que ya estaba en pantalla con lo que devolvió el catálogo completo.
 *
 * Los resultados locales conservan su orden y encabezan la lista; los del
 * catálogo se añaden debajo, de modo que cuando llega la respuesta del backend
 * ninguna fila visible se mueve ni cambia de sitio la selección del teclado. Un
 * resultado del catálogo que no coincida con el título se descarta: la fila
 * enseña el título y una que no se pueda señalar sólo confunde —y así tampoco
 * sobreviven los resultados de la consulta anterior mientras llega la nueva.
 */
export function mergeGameResults(
  local: readonly GameSummary[],
  catalog: readonly GameSummary[],
  query: string,
  extraLimit: number,
): PaletteGameResult[] {
  const results = local.map<PaletteGameResult>((game) => ({ game, fromCatalog: false }));
  const needle = normalizeSearchText(query.trim());
  if (!needle || !catalog.length || extraLimit <= 0) return results;
  const seen = new Set(local.map((game) => game.appId));
  const extras = catalog
    .filter((game) => !seen.has(game.appId))
    .map((game) => ({ game, score: fuzzyScore(gameHaystack(game), needle) }))
    .filter((candidate) => candidate.score >= MATCH_THRESHOLD)
    .sort((left, right) => right.score - left.score)
    .slice(0, extraLimit)
    .map<PaletteGameResult>(({ game }) => ({ game, fromCatalog: true }));
  return [...results, ...extras];
}
