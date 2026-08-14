import type { LibraryFilterChoice, LibraryFilterOptions } from "@/lib/types";

export interface LibraryFilters {
  statusId?: string;
  installed?: boolean;
  tracking?: boolean;
  earlyAccess?: boolean;
  neverPlayed?: boolean;
  minPlaytimeMinutes?: number;
  maxPlaytimeMinutes?: number;
  minProgress?: number;
  maxProgress?: number;
  minRating?: number;
  maxRating?: number;
  genre?: string;
  category?: string;
  tagId?: string;
  collectionId?: string;
  releaseFrom?: string;
  releaseTo?: string;
  lastPlayedFrom?: string;
  lastPlayedTo?: string;
  minAchievementPercent?: number;
  maxAchievementPercent?: number;
  steamDeckStatus?: string;
  targetDateFrom?: string;
  targetDateTo?: string;
  minSessionMinutes?: number;
  maxSessionMinutes?: number;
}

export type FilterChoice = LibraryFilterChoice;
export type { LibraryFilterOptions };

export interface FilterChipContext {
  statuses: FilterChoice[];
  collections: FilterChoice[];
  tags: FilterChoice[];
}

export interface LibraryFilterChip {
  key: keyof LibraryFilters;
  label: string;
  remove: (filters: LibraryFilters) => LibraryFilters;
}

const numberLimits: Partial<Record<keyof LibraryFilters, readonly [number, number]>> = {
  minPlaytimeMinutes: [0, 60_000_000],
  maxPlaytimeMinutes: [0, 60_000_000],
  minProgress: [0, 100],
  maxProgress: [0, 100],
  minRating: [1, 10],
  maxRating: [1, 10],
  minAchievementPercent: [0, 100],
  maxAchievementPercent: [0, 100],
  minSessionMinutes: [0, 60_000_000],
  maxSessionMinutes: [0, 60_000_000],
};

export function activeLibraryFilterCount(filters: LibraryFilters): number {
  return Object.values(filters).filter(
    (value) => value !== undefined && value !== null && value !== "",
  ).length;
}

export function normalizeLibraryFilters(filters: LibraryFilters): LibraryFilters {
  const normalized: LibraryFilters = {};
  for (const [rawKey, rawValue] of Object.entries(filters)) {
    const key = rawKey as keyof LibraryFilters;
    if (rawValue === undefined || rawValue === null || rawValue === "") continue;
    if (typeof rawValue === "string") {
      const value = rawValue.trim();
      if (value) Object.assign(normalized, { [key]: value });
      continue;
    }
    if (typeof rawValue === "number") {
      const limits = numberLimits[key];
      const value = limits
        ? Math.min(limits[1], Math.max(limits[0], Math.round(rawValue)))
        : rawValue;
      Object.assign(normalized, { [key]: value });
      continue;
    }
    Object.assign(normalized, { [key]: rawValue });
  }
  return normalized;
}

function removeKey(key: keyof LibraryFilters) {
  return (filters: LibraryFilters): LibraryFilters => ({ ...filters, [key]: undefined });
}

function choiceName(choices: FilterChoice[], id: string): string {
  return choices.find((choice) => choice.id === id)?.name ?? id;
}

function dateLabel(value: string): string {
  const [year, month, day] = value.split("-");
  return year && month && day ? `${day}/${month}/${year}` : value;
}

function hoursLabel(minutes: number): string {
  const hours = minutes / 60;
  return Number.isInteger(hours)
    ? `${hours.toLocaleString("es-ES")} h`
    : `${hours.toLocaleString("es-ES", { maximumFractionDigits: 1 })} h`;
}

function rangeChips(
  filters: LibraryFilters,
  minKey: keyof LibraryFilters,
  maxKey: keyof LibraryFilters,
  title: string,
  format: (value: number | string) => string,
): LibraryFilterChip[] {
  const min = filters[minKey] as number | string | undefined;
  const max = filters[maxKey] as number | string | undefined;
  const chips: LibraryFilterChip[] = [];
  if (min !== undefined) {
    chips.push({ key: minKey, label: `${title}: desde ${format(min)}`, remove: removeKey(minKey) });
  }
  if (max !== undefined) {
    chips.push({ key: maxKey, label: `${title}: hasta ${format(max)}`, remove: removeKey(maxKey) });
  }
  return chips;
}

export function filterChips(
  filters: LibraryFilters,
  context: FilterChipContext,
): LibraryFilterChip[] {
  const chips: LibraryFilterChip[] = [];
  if (filters.statusId) {
    chips.push({
      key: "statusId",
      label: `Estado: ${choiceName(context.statuses, filters.statusId)}`,
      remove: removeKey("statusId"),
    });
  }
  if (filters.installed !== undefined) {
    chips.push({
      key: "installed",
      label: filters.installed ? "Instalados" : "No instalados",
      remove: removeKey("installed"),
    });
  }
  if (filters.neverPlayed !== undefined) {
    chips.push({
      key: "neverPlayed",
      label: filters.neverPlayed ? "Nunca jugados" : "Ya jugados",
      remove: removeKey("neverPlayed"),
    });
  }
  chips.push(
    ...rangeChips(filters, "minPlaytimeMinutes", "maxPlaytimeMinutes", "Horas", (value) =>
      hoursLabel(Number(value)),
    ),
    ...rangeChips(filters, "minProgress", "maxProgress", "Progreso", (value) => `${value} %`),
    ...rangeChips(filters, "minRating", "maxRating", "Valoración", (value) => `${value}/10`),
  );
  if (filters.genre) {
    chips.push({ key: "genre", label: `Género: ${filters.genre}`, remove: removeKey("genre") });
  }
  if (filters.category) {
    chips.push({
      key: "category",
      label: `Categoría: ${filters.category}`,
      remove: removeKey("category"),
    });
  }
  if (filters.tagId) {
    chips.push({
      key: "tagId",
      label: `Etiqueta: ${choiceName(context.tags, filters.tagId)}`,
      remove: removeKey("tagId"),
    });
  }
  if (filters.collectionId) {
    chips.push({
      key: "collectionId",
      label: `Colección: ${choiceName(context.collections, filters.collectionId)}`,
      remove: removeKey("collectionId"),
    });
  }
  chips.push(
    ...rangeChips(filters, "releaseFrom", "releaseTo", "Lanzamiento", (value) =>
      dateLabel(String(value)),
    ),
    ...rangeChips(filters, "lastPlayedFrom", "lastPlayedTo", "Última partida", (value) =>
      dateLabel(String(value)),
    ),
    ...rangeChips(
      filters,
      "minAchievementPercent",
      "maxAchievementPercent",
      "Logros",
      (value) => `${value} %`,
    ),
  );
  if (filters.steamDeckStatus) {
    chips.push({
      key: "steamDeckStatus",
      label: `Steam Deck: ${filters.steamDeckStatus}`,
      remove: removeKey("steamDeckStatus"),
    });
  }
  if (filters.tracking !== undefined) {
    chips.push({
      key: "tracking",
      label: filters.tracking ? "En seguimiento" : "Sin seguimiento",
      remove: removeKey("tracking"),
    });
  }
  if (filters.earlyAccess !== undefined) {
    chips.push({
      key: "earlyAccess",
      label: filters.earlyAccess ? "Early Access" : "Fuera de Early Access",
      remove: removeKey("earlyAccess"),
    });
  }
  chips.push(
    ...rangeChips(filters, "targetDateFrom", "targetDateTo", "Fecha objetivo", (value) =>
      dateLabel(String(value)),
    ),
    ...rangeChips(filters, "minSessionMinutes", "maxSessionMinutes", "Sesión", (value) =>
      hoursLabel(Number(value)),
    ),
  );
  return chips;
}
