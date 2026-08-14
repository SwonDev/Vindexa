export interface PlannerMetricItem {
  appId: number;
  title: string;
  progress: number;
  estimatedMinutes?: number;
  plannedFor?: string;
  targetDate?: string;
  objective?: string;
}

export interface PlannerMetricColumn {
  id: string;
  name: string;
  wipLimit?: number;
  items: PlannerMetricItem[];
}

export interface PlannerMetricSettings {
  weeklyCapacityMinutes: number;
  monthlyCapacityMinutes: number;
}

export interface PlannerMetrics {
  totalGames: number;
  estimatedMinutes: number;
  remainingMinutes: number;
  averageProgress: number;
  wipOverloadedColumns: number;
  wipOverflowGames: number;
  weekPlannedMinutes: number;
  weekCapacityMinutes: number;
  weekOverloadMinutes: number;
  monthPlannedMinutes: number;
  monthCapacityMinutes: number;
  monthOverloadMinutes: number;
  unscheduledGames: number;
}

export interface PlannerPeriodSegment {
  date: string;
  label: string;
  items: PlannerMetricItem[];
}

export interface PlannerPeriodResult {
  rangeLabel: string;
  segments: PlannerPeriodSegment[];
  unscheduled: PlannerMetricItem[];
}

const MONTHS = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
const MONTHS_LONG = [
  "Enero",
  "Febrero",
  "Marzo",
  "Abril",
  "Mayo",
  "Junio",
  "Julio",
  "Agosto",
  "Septiembre",
  "Octubre",
  "Noviembre",
  "Diciembre",
];
const WEEKDAYS = ["Dom", "Lun", "Mar", "Mié", "Jue", "Vie", "Sáb"];

function parseIsoDate(value: string): Date | undefined {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return undefined;
  const parts = value.split("-").map(Number);
  const year = parts[0];
  const month = parts[1];
  const day = parts[2];
  if (year === undefined || month === undefined || day === undefined) return undefined;
  const date = new Date(Date.UTC(year, month - 1, day));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return undefined;
  }
  return date;
}

function monthShort(date: Date): string {
  return MONTHS[date.getUTCMonth()] ?? "";
}

function monthLong(date: Date): string {
  return MONTHS_LONG[date.getUTCMonth()] ?? "";
}

function toIsoDate(date: Date): string {
  return date.toISOString().slice(0, 10);
}

function addDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setUTCDate(next.getUTCDate() + days);
  return next;
}

function startOfWeek(date: Date): Date {
  const day = date.getUTCDay();
  return addDays(date, day === 0 ? -6 : 1 - day);
}

function uniqueItems(columns: PlannerMetricColumn[]): PlannerMetricItem[] {
  const seen = new Set<number>();
  return columns.flatMap((column) =>
    column.items.filter((item) => {
      if (seen.has(item.appId)) return false;
      seen.add(item.appId);
      return true;
    }),
  );
}

function estimatedMinutes(item: PlannerMetricItem): number {
  return Math.max(0, item.estimatedMinutes ?? 0);
}

function minutesWithin(items: PlannerMetricItem[], start: Date, end: Date): number {
  return items.reduce((total, item) => {
    const planned = item.plannedFor ? parseIsoDate(item.plannedFor) : undefined;
    return planned && planned >= start && planned <= end ? total + estimatedMinutes(item) : total;
  }, 0);
}

export function buildPlannerMetrics(
  columns: PlannerMetricColumn[],
  settings: PlannerMetricSettings,
  todayIso: string,
): PlannerMetrics {
  const today = parseIsoDate(todayIso);
  if (!today) throw new Error("La fecha de referencia del planificador no es válida.");
  const items = uniqueItems(columns);
  const weekStart = startOfWeek(today);
  const weekEnd = addDays(weekStart, 6);
  const monthStart = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), 1));
  const monthEnd = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth() + 1, 0));
  const weekPlannedMinutes = minutesWithin(items, weekStart, weekEnd);
  const monthPlannedMinutes = minutesWithin(items, monthStart, monthEnd);
  const wipOverflow = columns.reduce(
    (result, column) => {
      const overflow = column.wipLimit ? Math.max(0, column.items.length - column.wipLimit) : 0;
      return {
        columns: result.columns + Number(overflow > 0),
        games: result.games + overflow,
      };
    },
    { columns: 0, games: 0 },
  );

  return {
    totalGames: items.length,
    estimatedMinutes: items.reduce((sum, item) => sum + estimatedMinutes(item), 0),
    remainingMinutes: items.reduce(
      (sum, item) =>
        sum + Math.round((estimatedMinutes(item) * Math.max(0, 100 - item.progress)) / 100),
      0,
    ),
    averageProgress: items.length
      ? Math.round(items.reduce((sum, item) => sum + item.progress, 0) / items.length)
      : 0,
    wipOverloadedColumns: wipOverflow.columns,
    wipOverflowGames: wipOverflow.games,
    weekPlannedMinutes,
    weekCapacityMinutes: settings.weeklyCapacityMinutes,
    weekOverloadMinutes: Math.max(0, weekPlannedMinutes - settings.weeklyCapacityMinutes),
    monthPlannedMinutes,
    monthCapacityMinutes: settings.monthlyCapacityMinutes,
    monthOverloadMinutes: Math.max(0, monthPlannedMinutes - settings.monthlyCapacityMinutes),
    unscheduledGames: items.filter((item) => !item.plannedFor).length,
  };
}

function formatRange(start: Date, end: Date, includeYear: boolean): string {
  const sameMonth = start.getUTCMonth() === end.getUTCMonth();
  const startPart = sameMonth
    ? `${start.getUTCDate()}`
    : `${start.getUTCDate()} ${monthShort(start)}`;
  const yearPart = includeYear ? ` ${end.getUTCFullYear()}` : "";
  return `${startPart}–${end.getUTCDate()} ${monthShort(end)}${yearPart}`;
}

export function buildWeekSegments(
  columns: PlannerMetricColumn[],
  anchorIso: string,
): PlannerPeriodResult {
  const anchor = parseIsoDate(anchorIso);
  if (!anchor) throw new Error("La semana seleccionada no es válida.");
  const start = startOfWeek(anchor);
  const end = addDays(start, 6);
  const items = uniqueItems(columns);
  const byDate = new Map<string, PlannerMetricItem[]>();
  for (const item of items) {
    if (!item.plannedFor) continue;
    const planned = parseIsoDate(item.plannedFor);
    if (!planned || planned < start || planned > end) continue;
    const current = byDate.get(item.plannedFor) ?? [];
    current.push(item);
    byDate.set(item.plannedFor, current);
  }

  return {
    rangeLabel: formatRange(start, end, true),
    segments: Array.from({ length: 7 }, (_, index) => {
      const date = addDays(start, index);
      const iso = toIsoDate(date);
      return {
        date: iso,
        label: `${WEEKDAYS[date.getUTCDay()] ?? ""} ${date.getUTCDate()}`,
        items: byDate.get(iso) ?? [],
      };
    }),
    unscheduled: items.filter((item) => !item.plannedFor),
  };
}

export function buildMonthSegments(
  columns: PlannerMetricColumn[],
  anchorIso: string,
): PlannerPeriodResult {
  const anchor = parseIsoDate(anchorIso);
  if (!anchor) throw new Error("El mes seleccionado no es válido.");
  const monthStart = new Date(Date.UTC(anchor.getUTCFullYear(), anchor.getUTCMonth(), 1));
  const monthEnd = new Date(Date.UTC(anchor.getUTCFullYear(), anchor.getUTCMonth() + 1, 0));
  const calendarStart = startOfWeek(monthStart);
  const calendarEnd = addDays(startOfWeek(monthEnd), 6);
  const items = uniqueItems(columns);
  const monthItems = items.filter((item) => {
    const planned = item.plannedFor ? parseIsoDate(item.plannedFor) : undefined;
    return planned && planned >= monthStart && planned <= monthEnd;
  });
  const segments: PlannerPeriodSegment[] = [];
  for (let start = calendarStart; start <= calendarEnd; start = addDays(start, 7)) {
    const end = addDays(start, 6);
    segments.push({
      date: toIsoDate(start),
      label: formatRange(start, end, false),
      items: monthItems.filter((item) => {
        const planned = parseIsoDate(item.plannedFor ?? "");
        return planned && planned >= start && planned <= end;
      }),
    });
  }

  return {
    rangeLabel: `${monthLong(anchor)} ${anchor.getUTCFullYear()}`,
    segments,
    unscheduled: items.filter((item) => !item.plannedFor),
  };
}
