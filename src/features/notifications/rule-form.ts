/**
 * Estado, validación y traducción del formulario de reglas de aviso.
 *
 * Vive fuera del componente por dos motivos: se comparte entre el panel de la
 * pantalla de Seguimiento y la entrada rápida de la bandeja, y así la
 * validación se puede probar sin montar nada.
 *
 * ## Qué contrato replica
 *
 * Los mensajes reproducen literalmente los del backend (`db/notifications.rs`)
 * para que corregir el formulario y corregir el error del servidor sean la
 * misma acción. Hay una única regla en la que la interfaz es **más estricta**:
 * la fecha es obligatoria siempre.
 *
 * El backend admite guardar una regla sin fecha cuando el tipo no es `manual` y
 * no se repite, pero `due_rules` solo mira filas con `scheduled_for IS NOT NULL`
 * y la derivación de eventos oficiales no consulta reglas: una regla sin fecha
 * queda guardada y no se dispara jamás. Dejar crearla sería ofrecer un control
 * que no hace nada, así que aquí se exige la cita.
 */

import type {
  NotificationKind,
  NotificationRepeatRule,
  NotificationRule,
  SaveNotificationRuleInput,
} from "@/lib/types";

/** `MAX_TITLE_CHARS` de `db/notifications.rs`. */
export const MAX_RULE_TITLE = 120;
/** `MAX_BODY_CHARS` de `db/notifications.rs`. */
export const MAX_RULE_BODY = 2_000;
/** `MAX_LEAD_MINUTES`: treinta días. */
export const MAX_LEAD_MINUTES = 30 * 24 * 60;

export interface RuleKindOption {
  value: NotificationKind;
  label: string;
  /** Qué avisa exactamente. Se muestra bajo el selector, sin adornos. */
  hint: string;
  /** `NotificationKind::requires_game` del backend. */
  requiresGame: boolean;
}

/**
 * Los seis tipos que admite el `CHECK` de la migración 023, en el orden en que
 * tienen sentido para quien programa: primero el libre, después los anclados a
 * un juego y al final el resumen.
 */
export const RULE_KINDS: readonly RuleKindOption[] = [
  {
    value: "manual",
    label: "Aviso libre",
    hint: "Lo que tú escribas, el día y la hora que elijas.",
    requiresGame: false,
  },
  {
    value: "release_date",
    label: "Fecha de lanzamiento",
    hint: "Para la fecha que ya conoces de un juego de tu biblioteca.",
    requiresGame: true,
  },
  {
    value: "early_access_exit",
    label: "Salida de acceso anticipado",
    hint: "El día en que un juego deja Early Access.",
    requiresGame: true,
  },
  {
    value: "official_news",
    label: "Noticia oficial",
    hint: "Para revisar el feed oficial de un juego en una fecha concreta.",
    requiresGame: true,
  },
  {
    value: "dlc_release",
    label: "Contenido descargable",
    hint: "El día en que sale un DLC de un juego que ya tienes.",
    requiresGame: true,
  },
  {
    value: "reminder_digest",
    label: "Resumen de recordatorios",
    hint: "Un repaso periódico de lo que tienes pendiente.",
    requiresGame: false,
  },
];

export interface RepeatOption {
  value: NotificationRepeatRule;
  label: string;
}

export const REPEAT_RULES: readonly RepeatOption[] = [
  { value: "none", label: "No se repite" },
  { value: "daily", label: "Cada día" },
  { value: "weekly", label: "Cada semana" },
  { value: "monthly", label: "Cada mes" },
];

export interface LeadOption {
  value: number;
  label: string;
}

/** Márgenes habituales. Todos por debajo del techo de treinta días. */
export const LEAD_CHOICES: readonly LeadOption[] = [
  { value: 0, label: "A la hora exacta" },
  { value: 15, label: "15 minutos antes" },
  { value: 60, label: "1 hora antes" },
  { value: 12 * 60, label: "12 horas antes" },
  { value: 24 * 60, label: "1 día antes" },
  { value: 3 * 24 * 60, label: "3 días antes" },
  { value: 7 * 24 * 60, label: "1 semana antes" },
];

export interface RuleFormState {
  /** Vacío al crear; el identificador de la regla al editar. */
  id: string;
  kind: NotificationKind;
  appId: number | null;
  /** Título del juego elegido, solo para pintarlo sin volver a consultarlo. */
  gameTitle: string;
  title: string;
  body: string;
  /** Valor de un `input[type=datetime-local]`: `AAAA-MM-DDTHH:mm` en hora local. */
  scheduledForLocal: string;
  repeatRule: NotificationRepeatRule;
  leadMinutes: number;
  enabled: boolean;
}

export type RuleFormField =
  | "kind"
  | "appId"
  | "title"
  | "body"
  | "scheduledForLocal"
  | "leadMinutes";

export type RuleFormErrors = Partial<Record<RuleFormField, string>>;

export function ruleKind(kind: NotificationKind): RuleKindOption {
  return RULE_KINDS.find((option) => option.value === kind) ?? (RULE_KINDS[0] as RuleKindOption);
}

export function repeatLabel(rule: NotificationRepeatRule): string {
  return REPEAT_RULES.find((option) => option.value === rule)?.label ?? "No se repite";
}

export function leadLabel(minutes: number): string {
  if (minutes <= 0) return "A la hora exacta";
  const known = LEAD_CHOICES.find((option) => option.value === minutes);
  if (known) return known.label;
  if (minutes % (24 * 60) === 0) {
    const days = minutes / (24 * 60);
    return `${days} ${days === 1 ? "día" : "días"} antes`;
  }
  if (minutes % 60 === 0) {
    const hours = minutes / 60;
    return `${hours} ${hours === 1 ? "hora" : "horas"} antes`;
  }
  return `${minutes} minutos antes`;
}

/**
 * `Date` → valor de `datetime-local`, en la hora local de quien usa la
 * aplicación. `toISOString()` no sirve aquí: devolvería UTC y el campo mostraría
 * una hora que la persona no eligió.
 */
export function toLocalInputValue(iso: string | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}`;
}

/**
 * Valor de `datetime-local` → marca RFC-3339 con zona, que es lo único que
 * `parse_instant` acepta. Devuelve `null` cuando el texto no es una fecha.
 */
export function fromLocalInputValue(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const date = new Date(trimmed);
  if (Number.isNaN(date.getTime())) return null;
  return date.toISOString();
}

/** Fecha de un candidato (`2026-11-04`) → cita a las 09:00 de ese día. */
export function releaseDateToLocalInput(releaseDate: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(releaseDate.trim());
  if (!match) return "";
  return `${match[1]}-${match[2]}-${match[3]}T09:00`;
}

export function emptyRuleForm(): RuleFormState {
  return {
    id: "",
    kind: "manual",
    appId: null,
    gameTitle: "",
    title: "",
    body: "",
    scheduledForLocal: "",
    repeatRule: "none",
    leadMinutes: 0,
    enabled: true,
  };
}

export function ruleToForm(rule: NotificationRule): RuleFormState {
  return {
    id: rule.id,
    kind: rule.kind,
    appId: rule.appId ?? null,
    gameTitle: rule.gameTitle ?? "",
    title: rule.title,
    body: rule.body,
    scheduledForLocal: toLocalInputValue(rule.scheduledFor),
    repeatRule: rule.repeatRule,
    leadMinutes: rule.leadMinutes,
    enabled: rule.enabled,
  };
}

/**
 * Comprueba el formulario y devuelve un mensaje por campo que dice **qué
 * corregir**. Un objeto vacío significa que se puede guardar.
 */
export function validateRuleForm(state: RuleFormState): RuleFormErrors {
  const errors: RuleFormErrors = {};

  const title = state.title.trim();
  if (!title) {
    errors.title = "El aviso necesita un título: escribe qué quieres recordar.";
  } else if ([...title].length > MAX_RULE_TITLE) {
    const excess = [...title].length - MAX_RULE_TITLE;
    errors.title = `El título no puede superar ${MAX_RULE_TITLE} caracteres: sobran ${excess}.`;
  }

  const body = state.body.trim();
  if ([...body].length > MAX_RULE_BODY) {
    const excess = [...body].length - MAX_RULE_BODY;
    errors.body = `La descripción no puede superar ${MAX_RULE_BODY} caracteres: sobran ${excess}.`;
  }

  if (ruleKind(state.kind).requiresGame && !state.appId) {
    errors.appId =
      "Este tipo de aviso habla de un juego concreto: elige el juego antes de guardarlo.";
  }

  if (!state.scheduledForLocal.trim()) {
    errors.scheduledForLocal =
      "Sin fecha no hay cita y el aviso no se dispararía nunca: indica el día y la hora.";
  } else if (!fromLocalInputValue(state.scheduledForLocal)) {
    errors.scheduledForLocal =
      "Esa fecha no es válida: revisa el día y la hora antes de guardarla.";
  }

  if (!Number.isFinite(state.leadMinutes) || state.leadMinutes < 0) {
    errors.leadMinutes = "El margen de aviso no puede ser negativo: usa 0 o más minutos.";
  } else if (state.leadMinutes > MAX_LEAD_MINUTES) {
    errors.leadMinutes = `El margen de aviso no puede superar ${MAX_LEAD_MINUTES} minutos (30 días).`;
  }

  return errors;
}

/**
 * Aviso que no bloquea el guardado pero conviene decir: una primera cita en el
 * pasado con una regla que no se repite se dispara en el barrido siguiente.
 */
export function pastDateWarning(state: RuleFormState, now: Date = new Date()): string | null {
  const iso = fromLocalInputValue(state.scheduledForLocal);
  if (!iso) return null;
  const scheduled = new Date(iso).getTime() - state.leadMinutes * 60_000;
  if (scheduled > now.getTime()) return null;
  if (state.repeatRule === "none") {
    return "Esa fecha ya pasó: el aviso se enviará en cuanto Vindexa revise las citas pendientes.";
  }
  return "La primera cita ya pasó: cuenta como ancla y la próxima se calcula a partir de ella.";
}

/** Estado del formulario → entrada del comando. Recorta como hace el backend. */
export function formToInput(state: RuleFormState): SaveNotificationRuleInput {
  const scheduledFor = fromLocalInputValue(state.scheduledForLocal);
  const body = state.body.trim();
  return {
    ...(state.id ? { id: state.id } : {}),
    ...(state.appId ? { appId: state.appId } : {}),
    kind: state.kind,
    title: state.title.trim(),
    ...(body ? { body } : {}),
    ...(scheduledFor ? { scheduledFor } : {}),
    repeatRule: state.repeatRule,
    leadMinutes: state.leadMinutes,
    enabled: state.enabled,
  };
}

const dateTimeFormatter = new Intl.DateTimeFormat("es-ES", {
  dateStyle: "medium",
  timeStyle: "short",
});

export function formatRuleMoment(iso: string | undefined): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return dateTimeFormatter.format(date);
}

export interface NextOccurrenceCopy {
  /** Etiqueta corta del estado, para el ojo. */
  label: string;
  /** Frase completa, para el lector de pantalla y el `title`. */
  detail: string;
  state: "scheduled" | "paused" | "finished";
}

/**
 * Qué se pinta como «próximo aviso».
 *
 * Siempre `nextOccurrence`, nunca `scheduledFor`: el ancla es la primera cita
 * elegida y en una regla mensual del día 31 diría «31» aunque la cita real de
 * febrero caiga el 28.
 */
export function describeNextOccurrence(rule: NotificationRule): NextOccurrenceCopy {
  if (!rule.enabled) {
    return {
      label: "Pausado",
      detail: "El aviso está pausado: no se enviará hasta que vuelvas a activarlo.",
      state: "paused",
    };
  }
  if (rule.nextOccurrence) {
    const moment = formatRuleMoment(rule.nextOccurrence);
    const lead = rule.leadMinutes > 0 ? ` · ${leadLabel(rule.leadMinutes)}` : "";
    return {
      label: moment,
      detail: `Próximo aviso el ${moment}${lead}.`,
      state: "scheduled",
    };
  }
  return {
    label: "Sin próxima cita",
    detail: rule.lastFiredAt
      ? `Ya se envió el ${formatRuleMoment(rule.lastFiredAt)} y no se repite.`
      : "No hay ninguna cita futura calculada para este aviso.",
    state: "finished",
  };
}
