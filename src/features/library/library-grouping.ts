import type { GameSummary } from "@/lib/types";

/**
 * Agrupación de la biblioteca.
 *
 * Ordenar reparte los juegos, pero deja una única lista continua en la que no
 * hay forma de situarse. Los cortes de aquí son lo que da referencias por las
 * que orientarse y destinos a los que saltar.
 */
export type LibraryGrouping =
  | "none"
  | "initial"
  | "status"
  | "genre"
  | "releaseYear"
  | "developer"
  | "lastPlayed";

export interface LibraryGroupOption {
  id: LibraryGrouping;
  label: string;
  /** Qué responde este corte, para que la elección no sea a ciegas. */
  hint: string;
}

export const LIBRARY_GROUPINGS: readonly LibraryGroupOption[] = [
  { id: "none", label: "Sin agrupar", hint: "Una sola lista continua." },
  { id: "initial", label: "Inicial", hint: "Por la primera letra del título." },
  { id: "status", label: "Estado", hint: "Jugando, terminados, pendientes…" },
  { id: "genre", label: "Género", hint: "Por el primer género declarado." },
  { id: "releaseYear", label: "Año de lanzamiento", hint: "Por año de publicación." },
  { id: "developer", label: "Estudio", hint: "Por quien lo desarrolló." },
  { id: "lastPlayed", label: "Última vez jugado", hint: "Hoy, esta semana, este mes…" },
] as const;

export interface LibraryGroup {
  /** Clave estable: es el destino que usa el índice de salto. */
  key: string;
  label: string;
  games: GameSummary[];
}

const SIN_DATO = "Sin dato";

/**
 * Inicial del título ignorando artículos y acentos.
 *
 * «Él» y «El» caen en la misma letra, y cualquier título que empiece por dígito
 * o símbolo va a un grupo `#` en lugar de crear veintitantos grupos de uno.
 */
export function titleInitial(title: string): string {
  const normalized = title
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .trim()
    .replace(/^(the|el|la|los|las|un|una|a|an)\s+/i, "");
  const first = normalized.charAt(0).toUpperCase();
  return /^[A-Z]$/.test(first) ? first : "#";
}

/** Antigüedad legible de la última sesión, en cortes con sentido. */
export function lastPlayedBucket(value: string | undefined, now: Date): string {
  if (!value) return "Nunca jugado";
  const played = new Date(value);
  if (Number.isNaN(played.getTime())) return "Nunca jugado";
  const days = Math.floor((now.getTime() - played.getTime()) / 86_400_000);
  if (days <= 0) return "Hoy";
  if (days <= 7) return "Esta semana";
  if (days <= 30) return "Este mes";
  if (days <= 90) return "Últimos tres meses";
  if (days <= 365) return "Este año";
  return "Hace más de un año";
}

/** Orden en el que se presentan los cortes de antigüedad. */
const LAST_PLAYED_ORDER = [
  "Hoy",
  "Esta semana",
  "Este mes",
  "Últimos tres meses",
  "Este año",
  "Hace más de un año",
  "Nunca jugado",
];

function groupKeyFor(game: GameSummary, grouping: LibraryGrouping, now: Date): string {
  switch (grouping) {
    case "initial":
      return titleInitial(game.title);
    case "status":
      return game.statusName || SIN_DATO;
    case "genre":
      return game.genres?.[0] ?? SIN_DATO;
    case "releaseYear": {
      const year = game.releaseDate?.slice(0, 4);
      return year && /^\d{4}$/.test(year) ? year : SIN_DATO;
    }
    case "developer":
      return game.developer || SIN_DATO;
    case "lastPlayed":
      return lastPlayedBucket(game.lastPlayedAt, now);
    default:
      return "";
  }
}

/**
 * Parte la página cargada en grupos **conservando el orden recibido**.
 *
 * El orden lo decide el backend; agrupar no puede reordenar por su cuenta o la
 * ordenación elegida dejaría de significar nada. Lo único que se ordena es la
 * secuencia de los propios grupos, y solo cuando tiene una lectura natural
 * (alfabética, cronológica o por antigüedad).
 */
export function groupLibrary(
  games: readonly GameSummary[],
  grouping: LibraryGrouping,
  now: Date = new Date(),
): LibraryGroup[] {
  if (grouping === "none" || games.length === 0) return [];

  const buckets = new Map<string, GameSummary[]>();
  for (const game of games) {
    const key = groupKeyFor(game, grouping, now);
    const bucket = buckets.get(key);
    if (bucket) bucket.push(game);
    else buckets.set(key, [game]);
  }

  const entries = [...buckets.entries()];
  if (grouping === "lastPlayed") {
    entries.sort((a, b) => LAST_PLAYED_ORDER.indexOf(a[0]) - LAST_PLAYED_ORDER.indexOf(b[0]));
  } else if (grouping === "releaseYear") {
    // Lo más reciente primero; lo que no tiene año, al final.
    entries.sort(([a], [b]) => {
      if (a === SIN_DATO) return 1;
      if (b === SIN_DATO) return -1;
      return Number(b) - Number(a);
    });
  } else if (grouping === "initial" || grouping === "genre" || grouping === "developer") {
    entries.sort(([a], [b]) => {
      if (a === SIN_DATO || a === "#") return 1;
      if (b === SIN_DATO || b === "#") return -1;
      return a.localeCompare(b, "es");
    });
  }

  return entries.map(([key, bucket]) => ({ key, label: key, games: bucket }));
}

/** Etiquetas del índice de salto, en el mismo orden que los grupos. */
export function groupIndex(groups: readonly LibraryGroup[]): { key: string; label: string }[] {
  return groups.map((group) => ({
    key: group.key,
    // El índice de iniciales cabe en una columna estrecha; el resto se acorta.
    label: group.label.length <= 4 ? group.label : group.label.slice(0, 3).toUpperCase(),
  }));
}
