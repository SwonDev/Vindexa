/**
 * Vistas guardadas y su **combinación**.
 *
 * Otras aplicaciones del ramo guardan presets pero solo dejan aplicar uno cada
 * vez: elegir el segundo descarta el primero. Aquí una vista se puede apilar
 * sobre otra, y el resultado es la **intersección** de ambas —los juegos que
 * cumplen las dos condiciones—, no la sustitución de una por otra.
 *
 * Intersecar no siempre es posible, y ese es el punto delicado del módulo:
 * `statusId` es un campo único, así que «En curso» y «Terminados» no pueden
 * cumplirse a la vez. En lugar de elegir en silencio, la combinación devuelve
 * los conflictos por escrito para que la interfaz los enseñe y la persona
 * decida. Manda el valor de la vista aplicada más tarde, que es lo que acaba de
 * pedir.
 */

import type { LibraryFilters } from "@/features/library/library-filters";
import type { LibraryGrouping } from "@/features/library/library-grouping";
import type { GameSort, LibraryView } from "@/lib/types";

/** Instantánea de consulta que guarda una vista. */
export interface SavedViewQuery {
  search?: string;
  sort?: GameSort;
  grouping?: LibraryGrouping;
  view?: LibraryView;
  filters?: LibraryFilters;
}

export interface SavedLibraryView {
  id: string;
  name: string;
  description: string;
  icon: string;
  accent: string;
  query: SavedViewQuery;
  pinned: boolean;
  position: number;
  lastUsedAt: string | null;
  useCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface SaveViewInput {
  id?: string;
  name: string;
  description?: string;
  icon?: string;
  accent?: string;
  query: SavedViewQuery;
  pinned?: boolean;
}

/** Un campo que dos vistas piden con valores incompatibles. */
export interface ViewConflict {
  field: keyof LibraryFilters;
  label: string;
  /** Lo que pedía la vista de abajo, descartado. */
  discarded: string;
  /** Lo que pedía la vista de arriba, que es lo que se aplica. */
  kept: string;
  reason: "single-value" | "empty-range";
}

export interface CombinedView {
  filters: LibraryFilters;
  search: string;
  conflicts: ViewConflict[];
}

/**
 * Campos que solo admiten un valor. Intersecarlos es imposible cuando difieren:
 * ningún juego tiene dos estados ni dos géneros primarios a la vez.
 */
const singleValueFields = {
  statusId: "Estado",
  genre: "Género",
  category: "Categoría",
  tagId: "Etiqueta",
  collectionId: "Colección",
  steamDeckStatus: "Compatibilidad con Steam Deck",
  drmState: "Protección anticopia (DRM)",
  installed: "Instalación",
  tracking: "Seguimiento",
  earlyAccess: "Acceso anticipado",
  neverPlayed: "Sin estrenar",
} as const satisfies Partial<Record<keyof LibraryFilters, string>>;

/** Rangos numéricos: se intersecan estrechando por ambos extremos. */
const numericRanges = [
  { min: "minPlaytimeMinutes", max: "maxPlaytimeMinutes", label: "Tiempo jugado" },
  { min: "minProgress", max: "maxProgress", label: "Progreso" },
  { min: "minRating", max: "maxRating", label: "Nota" },
  { min: "minAchievementPercent", max: "maxAchievementPercent", label: "Logros" },
  { min: "minSessionMinutes", max: "maxSessionMinutes", label: "Duración de sesión" },
] as const satisfies readonly {
  min: keyof LibraryFilters;
  max: keyof LibraryFilters;
  label: string;
}[];

/** Rangos de fecha en ISO, comparables lexicográficamente. */
const dateRanges = [
  { min: "releaseFrom", max: "releaseTo", label: "Fecha de salida" },
  { min: "lastPlayedFrom", max: "lastPlayedTo", label: "Última partida" },
  { min: "targetDateFrom", max: "targetDateTo", label: "Fecha objetivo" },
] as const satisfies readonly {
  min: keyof LibraryFilters;
  max: keyof LibraryFilters;
  label: string;
}[];

function describe(value: unknown): string {
  if (value === true) return "sí";
  if (value === false) return "no";
  return String(value);
}

function isSet(value: unknown): boolean {
  return value !== undefined && value !== null && value !== "";
}

/**
 * Interseca dos conjuntos de filtros. `next` es la vista que se acaba de
 * aplicar y gana los empates irresolubles.
 */
export function intersectFilters(
  base: LibraryFilters,
  next: LibraryFilters,
): { filters: LibraryFilters; conflicts: ViewConflict[] } {
  const filters: LibraryFilters = { ...base, ...next };
  const conflicts: ViewConflict[] = [];

  for (const [field, label] of Object.entries(singleValueFields) as [
    keyof LibraryFilters,
    string,
  ][]) {
    const previous = base[field];
    const incoming = next[field];
    if (!isSet(previous) || !isSet(incoming) || previous === incoming) continue;
    conflicts.push({
      field,
      label,
      discarded: describe(previous),
      kept: describe(incoming),
      reason: "single-value",
    });
  }

  for (const range of [...numericRanges, ...dateRanges]) {
    // El extremo inferior se queda con el más exigente de los dos, y el
    // superior igual: así el resultado nunca es más ancho que sus partes.
    const lows = [base[range.min], next[range.min]].filter(isSet);
    const highs = [base[range.max], next[range.max]].filter(isSet);
    const low = lows.length
      ? lows.reduce((a, b) => ((a as number | string) > (b as number | string) ? a : b))
      : undefined;
    const high = highs.length
      ? highs.reduce((a, b) => ((a as number | string) < (b as number | string) ? a : b))
      : undefined;

    if (low === undefined) delete filters[range.min];
    else Object.assign(filters, { [range.min]: low });
    if (high === undefined) delete filters[range.max];
    else Object.assign(filters, { [range.max]: high });

    if (
      low !== undefined &&
      high !== undefined &&
      (low as number | string) > (high as number | string)
    ) {
      // La intersección quedó vacía: ningún juego puede caer en ambos tramos.
      // Se conserva el tramo de la vista recién aplicada, que es lo que la
      // persona acaba de pedir, y se avisa de lo descartado.
      conflicts.push({
        field: range.min,
        label: range.label,
        discarded: `${describe(base[range.min] ?? "…")} – ${describe(base[range.max] ?? "…")}`,
        kept: `${describe(next[range.min] ?? "…")} – ${describe(next[range.max] ?? "…")}`,
        reason: "empty-range",
      });
      if (isSet(next[range.min])) Object.assign(filters, { [range.min]: next[range.min] });
      else delete filters[range.min];
      if (isSet(next[range.max])) Object.assign(filters, { [range.max]: next[range.max] });
      else delete filters[range.max];
    }
  }

  for (const key of Object.keys(filters) as (keyof LibraryFilters)[]) {
    if (!isSet(filters[key])) delete filters[key];
  }
  return { filters, conflicts };
}

/**
 * Combina varias vistas en el orden en que se aplicaron. La última manda en lo
 * que no se puede intersecar.
 */
export function combineViews(views: readonly SavedLibraryView[]): CombinedView {
  let filters: LibraryFilters = {};
  const conflicts: ViewConflict[] = [];
  const searches: string[] = [];

  for (const view of views) {
    const result = intersectFilters(filters, view.query.filters ?? {});
    filters = result.filters;
    conflicts.push(...result.conflicts);
    const search = view.query.search?.trim();
    if (search && !searches.includes(search)) searches.push(search);
  }

  return { filters, search: searches.join(" "), conflicts };
}

/**
 * Presentación de la combinación: la última vista aplicada decide orden,
 * agrupación y modo de vista, porque son decisiones de presentación y no
 * tienen intersección posible.
 */
export function combinedPresentation(views: readonly SavedLibraryView[]): {
  sort?: GameSort;
  grouping?: LibraryGrouping;
  view?: LibraryView;
} {
  const result: { sort?: GameSort; grouping?: LibraryGrouping; view?: LibraryView } = {};
  for (const view of views) {
    if (view.query.sort) result.sort = view.query.sort;
    if (view.query.grouping) result.grouping = view.query.grouping;
    if (view.query.view) result.view = view.query.view;
  }
  return result;
}

/** Alterna una vista dentro de la pila activa conservando el orden de llegada. */
export function toggleViewInStack(stack: readonly string[], id: string): string[] {
  return stack.includes(id) ? stack.filter((entry) => entry !== id) : [...stack, id];
}

/**
 * ¿La consulta actual coincide exactamente con lo que guarda la vista? Sirve
 * para marcar una vista como «tal cual está» y ofrecer «Actualizar» cuando no.
 */
export function queryMatchesView(query: SavedViewQuery, view: SavedLibraryView): boolean {
  return normalizeQuery(query) === normalizeQuery(view.query);
}

/** Serializa una consulta con las claves ordenadas para poder compararla. */
export function normalizeQuery(query: SavedViewQuery): string {
  const filters = query.filters ?? {};
  const entries = (Object.keys(filters) as (keyof LibraryFilters)[])
    .filter((key) => isSet(filters[key]))
    .sort()
    .map((key) => [key, filters[key]] as const);
  return JSON.stringify({
    search: query.search?.trim() ?? "",
    sort: query.sort ?? "",
    grouping: query.grouping ?? "",
    view: query.view ?? "",
    filters: entries,
  });
}

/**
 * Resume una vista en una línea legible, para el listado y para la paleta de
 * comandos. Sin esto, una vista es un nombre sin contenido visible.
 */
export function describeView(
  view: SavedLibraryView,
  context: {
    statuses?: Map<string, string>;
    collections?: Map<string, string>;
    tags?: Map<string, string>;
  },
): string {
  const parts: string[] = [];
  const filters = view.query.filters ?? {};
  if (view.query.search?.trim()) parts.push(`«${view.query.search.trim()}»`);
  if (filters.statusId) parts.push(context.statuses?.get(filters.statusId) ?? filters.statusId);
  if (filters.collectionId) {
    parts.push(context.collections?.get(filters.collectionId) ?? filters.collectionId);
  }
  if (filters.tagId) parts.push(context.tags?.get(filters.tagId) ?? filters.tagId);
  if (filters.genre) parts.push(filters.genre);
  if (filters.category) parts.push(filters.category);
  if (filters.installed === true) parts.push("instalados");
  if (filters.installed === false) parts.push("sin instalar");
  if (filters.tracking) parts.push("en seguimiento");
  if (filters.earlyAccess) parts.push("acceso anticipado");
  if (filters.neverPlayed) parts.push("sin estrenar");
  if (filters.minRating !== undefined) parts.push(`nota ≥ ${filters.minRating}`);
  if (filters.minProgress !== undefined) parts.push(`progreso ≥ ${filters.minProgress} %`);
  if (filters.maxProgress !== undefined) parts.push(`progreso ≤ ${filters.maxProgress} %`);
  if (parts.length === 0) return "Toda la biblioteca";
  return parts.join(" · ");
}
