const integer = new Intl.NumberFormat("es-ES", { maximumFractionDigits: 0 });
const relative = new Intl.RelativeTimeFormat("es-ES", { numeric: "auto" });

export function formatPlaytime(minutes: number): string {
  if (minutes < 60) return `${integer.format(minutes)} min`;
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  return remainder ? `${integer.format(hours)} h ${remainder} min` : `${integer.format(hours)} h`;
}

export function formatBytes(bytes?: number): string {
  if (!bytes) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  // Coma decimal: la interfaz está en español y un punto se lee como millar.
  const rounded = value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1);
  return `${rounded.replace(".", ",")} ${units[index]}`;
}

export function formatDate(value?: string): string {
  if (!value) return "Nunca";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("es-ES", {
    day: "2-digit",
    month: "short",
    year: "numeric",
  }).format(date);
}

export function formatRelativeDate(value?: string): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const minutes = Math.round((date.getTime() - Date.now()) / 60_000);
  if (Math.abs(minutes) < 60) return relative.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 48) return relative.format(hours, "hour");
  const days = Math.round(hours / 24);
  if (Math.abs(days) < 30) return relative.format(days, "day");
  return formatDate(value);
}

export function initials(title: string): string {
  return title
    .split(/\s+/)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("");
}

/**
 * La compatibilidad con Steam Deck llega de la API oficial en inglés
 * (`verified`, `playable`, `unsupported`, `unknown`). Vindexa la presenta en
 * español sin reinterpretarla: un valor desconocido se muestra tal cual en
 * lugar de inventar una traducción.
 */
const STEAM_DECK_LABELS: Record<string, string> = {
  verified: "Verificado",
  playable: "Jugable",
  unsupported: "No compatible",
  unknown: "Sin comprobar",
};

export function formatSteamDeckStatus(value?: string): string | undefined {
  if (!value) return undefined;
  return STEAM_DECK_LABELS[value.trim().toLowerCase()] ?? value;
}
