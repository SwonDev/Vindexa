import {
  IconCirclePlus,
  IconLoader2,
  IconPlus,
  IconSparkles,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { CollectionIcon, CollectionIconPicker } from "@/features/collections/CollectionIcon";
import { formatDate, formatPlaytime } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  CollectionSummary,
  SaveCollectionInput,
  SmartRule,
  StatusDefinition,
} from "@/lib/types";

/**
 * Semilla de una colección sugerida por la pantalla. Rellena el diálogo sin
 * guardar nada: la persona sigue viendo, editando y confirmando sus reglas.
 */
export interface CollectionSeed {
  name: string;
  description: string;
  color: string;
  icon: string;
  kind: "manual" | "smart";
  matchMode: "all" | "any";
  rules: readonly Omit<SmartRule, "id" | "position">[];
}

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  collection?: CollectionSummary | undefined;
  statuses?: StatusDefinition[] | undefined;
  /** Plantilla con la que arrancar una colección nueva. Se ignora al editar. */
  seed?: CollectionSeed | undefined;
}

type RuleFieldKind = "boolean" | "number" | "date" | "status" | "text" | "list";

interface RuleFieldOption {
  value: string;
  label: string;
  kind: RuleFieldKind;
  operators: readonly string[];
  defaultOperator: string;
  defaultValue: unknown;
}

const booleanOperators = ["equals", "notEquals", "isTrue", "isFalse"] as const;
const numberOperators = [
  "equals",
  "notEquals",
  "greaterThan",
  "greaterOrEqual",
  "lessThan",
  "lessOrEqual",
  "isSet",
  "isNotSet",
] as const;
const dateOperators = ["equals", "notEquals", "before", "after", "isSet", "isNotSet"] as const;
const textOperators = [
  "equals",
  "notEquals",
  "contains",
  "notContains",
  "in",
  "isSet",
  "isNotSet",
] as const;
const listOperators = ["equals", "notEquals", "contains", "notContains", "in"] as const;

const installedFieldOption: RuleFieldOption = {
  value: "installed",
  label: "Instalación",
  kind: "boolean",
  operators: booleanOperators,
  defaultOperator: "equals",
  defaultValue: true,
};

const fieldOptions: readonly RuleFieldOption[] = [
  installedFieldOption,
  {
    value: "tracking",
    label: "Seguimiento",
    kind: "boolean",
    operators: booleanOperators,
    defaultOperator: "equals",
    defaultValue: true,
  },
  {
    value: "earlyAccess",
    label: "Early Access",
    kind: "boolean",
    operators: booleanOperators,
    defaultOperator: "equals",
    defaultValue: true,
  },
  {
    value: "isFree",
    label: "Juego gratuito",
    kind: "boolean",
    operators: booleanOperators,
    defaultOperator: "equals",
    defaultValue: true,
  },
  {
    value: "progress",
    label: "Progreso (%)",
    kind: "number",
    operators: numberOperators,
    defaultOperator: "greaterOrEqual",
    defaultValue: 75,
  },
  {
    value: "priority",
    label: "Prioridad",
    kind: "number",
    operators: numberOperators,
    defaultOperator: "greaterOrEqual",
    defaultValue: 1,
  },
  {
    value: "rating",
    label: "Valoración",
    kind: "number",
    operators: numberOperators,
    defaultOperator: "greaterOrEqual",
    defaultValue: 1,
  },
  {
    value: "playtimeMinutes",
    label: "Tiempo jugado (min)",
    kind: "number",
    operators: numberOperators,
    defaultOperator: "greaterOrEqual",
    defaultValue: 60,
  },
  {
    value: "estimatedMinutes",
    label: "Duración estimada (min)",
    kind: "number",
    operators: numberOperators,
    defaultOperator: "lessOrEqual",
    defaultValue: 600,
  },
  {
    value: "achievementPercent",
    label: "Logros (%)",
    kind: "number",
    operators: numberOperators,
    defaultOperator: "greaterOrEqual",
    defaultValue: 50,
  },
  {
    value: "targetDate",
    label: "Fecha objetivo",
    kind: "date",
    operators: dateOperators,
    defaultOperator: "before",
    defaultValue: "",
  },
  {
    value: "releaseDate",
    label: "Fecha de lanzamiento",
    kind: "date",
    operators: dateOperators,
    defaultOperator: "after",
    defaultValue: "",
  },
  {
    value: "lastPlayedAt",
    label: "Última partida",
    kind: "date",
    operators: dateOperators,
    defaultOperator: "after",
    defaultValue: "",
  },
  {
    value: "importedAt",
    label: "Fecha de importación",
    kind: "date",
    operators: dateOperators,
    defaultOperator: "after",
    defaultValue: "",
  },
  {
    value: "updatedAt",
    label: "Última actualización",
    kind: "date",
    operators: dateOperators,
    defaultOperator: "after",
    defaultValue: "",
  },
  {
    value: "statusId",
    label: "Estado",
    kind: "status",
    operators: textOperators,
    defaultOperator: "equals",
    defaultValue: "",
  },
  {
    value: "title",
    label: "Título",
    kind: "text",
    operators: textOperators,
    defaultOperator: "contains",
    defaultValue: "",
  },
  {
    value: "developer",
    label: "Desarrollador",
    kind: "text",
    operators: textOperators,
    defaultOperator: "contains",
    defaultValue: "",
  },
  {
    value: "publisher",
    label: "Editor",
    kind: "text",
    operators: textOperators,
    defaultOperator: "contains",
    defaultValue: "",
  },
  {
    value: "steamDeckStatus",
    label: "Compatibilidad Steam Deck",
    kind: "text",
    operators: textOperators,
    defaultOperator: "equals",
    defaultValue: "verified",
  },
  {
    value: "genre",
    label: "Género",
    kind: "list",
    operators: listOperators,
    defaultOperator: "contains",
    defaultValue: "",
  },
  {
    value: "category",
    label: "Categoría",
    kind: "list",
    operators: listOperators,
    defaultOperator: "contains",
    defaultValue: "",
  },
  {
    value: "tag",
    label: "Etiqueta personal",
    kind: "list",
    operators: listOperators,
    defaultOperator: "contains",
    defaultValue: "",
  },
];

const operatorLabels: Record<string, string> = {
  equals: "es",
  notEquals: "no es",
  greaterThan: "mayor que",
  greaterOrEqual: "mayor o igual",
  lessThan: "menor que",
  lessOrEqual: "menor o igual",
  contains: "contiene",
  notContains: "no contiene",
  in: "está en la lista",
  before: "antes de",
  after: "después de",
  isSet: "está definido",
  isNotSet: "no está definido",
  isTrue: "es verdadero",
  isFalse: "es falso",
};

const operatorsWithoutValue = new Set(["isSet", "isNotSet", "isTrue", "isFalse"]);

/* --- Resumen legible de las reglas --------------------------------------
 *
 * Una colección inteligente no se explica con la palabra «Inteligente»: hay que
 * poder leer qué la mantiene al día sin abrir el editor. Estas funciones
 * traducen el contrato persistido a frases cortas reutilizando el mismo
 * catálogo de campos y operadores que usa el formulario, para que editor y
 * resumen nunca puedan divergir.
 */

const integerFormat = new Intl.NumberFormat("es-ES", { maximumFractionDigits: 2 });
const percentFields = new Set(["progress", "achievementPercent"]);
const minuteFields = new Set(["playtimeMinutes", "estimatedMinutes"]);
const lowerBoundOperators = new Set(["greaterThan", "greaterOrEqual"]);
const upperBoundOperators = new Set(["lessThan", "lessOrEqual"]);

/** Frases completas para los campos booleanos: «Instalados» dice más que «Instalación: sí». */
const booleanPhrases: Record<string, readonly [string, string]> = {
  installed: ["Instalados", "Sin instalar"],
  tracking: ["En seguimiento", "Fuera de seguimiento"],
  earlyAccess: ["En acceso anticipado", "Sin acceso anticipado"],
  isFree: ["Gratuitos", "De pago"],
};

function shortFieldLabel(option: RuleFieldOption): string {
  return option.label.replace(/\s*\([^)]*\)$/, "");
}

function formatRuleValue(
  option: RuleFieldOption,
  value: unknown,
  statuses: readonly StatusDefinition[],
): string {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === "string").join(", ");
  }
  if (option.kind === "status") {
    const status = statuses.find((candidate) => candidate.id === value);
    return status?.name ?? String(value ?? "");
  }
  if (option.kind === "date") return formatDate(typeof value === "string" ? value : undefined);
  if (option.kind === "number" && typeof value === "number") {
    if (minuteFields.has(option.value)) return formatPlaytime(value);
    return percentFields.has(option.value)
      ? `${integerFormat.format(value)} %`
      : integerFormat.format(value);
  }
  return String(value ?? "");
}

function describeRule(rule: SmartRule, statuses: readonly StatusDefinition[]): string | undefined {
  const option = fieldOptions.find((candidate) => candidate.value === rule.field);
  if (!option) return undefined;
  const label = shortFieldLabel(option);

  if (option.kind === "boolean") {
    const phrases = booleanPhrases[option.value];
    const affirmative =
      rule.operator === "isTrue" || (rule.operator === "equals" && rule.value === true);
    const negative =
      rule.operator === "isFalse" || (rule.operator === "equals" && rule.value === false);
    if (phrases && (affirmative || negative)) return affirmative ? phrases[0] : phrases[1];
    if (affirmative || negative) return `${label}: ${affirmative ? "sí" : "no"}`;
  }

  if (rule.operator === "isSet") return `${label} definido`;
  if (rule.operator === "isNotSet") return `${label} sin definir`;

  const value = formatRuleValue(option, rule.value, statuses);
  if (!value) return undefined;
  switch (rule.operator) {
    case "equals":
      return `${label}: ${value}`;
    case "notEquals":
      return `${label} distinto de ${value}`;
    case "greaterThan":
      return `${label} > ${value}`;
    case "greaterOrEqual":
      return `${label} ≥ ${value}`;
    case "lessThan":
      return `${label} < ${value}`;
    case "lessOrEqual":
      return `${label} ≤ ${value}`;
    case "before":
      return `${label} antes del ${value}`;
    case "after":
      return `${label} después del ${value}`;
    case "contains":
      return `${label} contiene «${value}»`;
    case "notContains":
      return `${label} sin «${value}»`;
    case "in":
      return `${label}: ${value}`;
    default:
      return `${label}: ${value}`;
  }
}

/**
 * Convierte las reglas guardadas en frases cortas y ordenadas por grupo. Dos
 * cotas del mismo campo dentro de un grupo se funden en un intervalo, de modo
 * que se lee «Progreso entre 20 % y 80 %» en vez de dos condiciones sueltas.
 */
export function describeSmartRules(
  rules: readonly SmartRule[],
  statuses: readonly StatusDefinition[] = [],
): string[] {
  const groups = new Map<number, SmartRule[]>();
  for (const rule of rules) {
    const group = groups.get(rule.groupId);
    if (group) group.push(rule);
    else groups.set(rule.groupId, [rule]);
  }

  const phrases: string[] = [];
  for (const groupId of Array.from(groups.keys()).sort((a, b) => a - b)) {
    const groupRules = (groups.get(groupId) ?? []).slice().sort((a, b) => a.position - b.position);
    const consumed = new Set<SmartRule>();
    for (const rule of groupRules) {
      if (consumed.has(rule)) continue;
      const option = fieldOptions.find((candidate) => candidate.value === rule.field);
      if (option && lowerBoundOperators.has(rule.operator)) {
        const upper = groupRules.find(
          (candidate) =>
            candidate !== rule &&
            !consumed.has(candidate) &&
            candidate.field === rule.field &&
            upperBoundOperators.has(candidate.operator),
        );
        if (upper) {
          consumed.add(rule);
          consumed.add(upper);
          const from = formatRuleValue(option, rule.value, statuses);
          const to = formatRuleValue(option, upper.value, statuses);
          if (from && to) {
            phrases.push(`${shortFieldLabel(option)} entre ${from} y ${to}`);
            continue;
          }
        }
      }
      consumed.add(rule);
      const phrase = describeRule(rule, statuses);
      if (phrase) phrases.push(phrase);
    }
  }
  return phrases;
}

function ruleHasValidValue(rule: SmartRule) {
  const field = fieldOptions.find((option) => option.value === rule.field);
  if (!field?.operators.includes(rule.operator)) return false;
  if (operatorsWithoutValue.has(rule.operator)) return true;
  if (field.kind === "boolean") return typeof rule.value === "boolean";
  if (field.kind === "number") {
    return typeof rule.value === "number" && Number.isFinite(rule.value);
  }
  if (Array.isArray(rule.value)) {
    return rule.value.some((value) => typeof value === "string" && value.trim().length > 0);
  }
  return typeof rule.value === "string" && rule.value.trim().length > 0;
}

function defaultRule(): SmartRule {
  return {
    id: crypto.randomUUID(),
    groupId: 0,
    field: "installed",
    operator: "equals",
    value: true,
    position: 0,
  };
}

function seedRules(seed: CollectionSeed | undefined): SmartRule[] {
  if (!seed || seed.kind === "manual") return [];
  return seed.rules.map((rule, index) => ({
    ...rule,
    id: crypto.randomUUID(),
    position: index,
  }));
}

export function CollectionEditorDialog({
  open,
  onOpenChange,
  collection,
  statuses = [],
  seed,
}: Props) {
  const queryClient = useQueryClient();
  const editing = Boolean(collection);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [color, setColor] = useState("#5CAAC1");
  const [icon, setIcon] = useState("sparkles");
  const [kind, setKind] = useState<"manual" | "smart">("smart");
  const [matchMode, setMatchMode] = useState<"all" | "any">("all");
  const [rules, setRules] = useState<SmartRule[]>([defaultRule()]);
  const [error, setError] = useState<string>();

  /* Comparte caché con el resumen de reglas de la pantalla de colecciones: si
   * la tarjeta ya las leyó, el editor abre con ellas y sin pedir nada. Una
   * lectura fallida sigue reintentándose al abrir, porque abrir el editor es
   * justo el momento en el que volver a intentarlo tiene sentido. */
  const rulesQuery = useQuery({
    queryKey: ["collection-rules", collection?.id],
    queryFn: () => api.listSmartRules(collection?.id ?? ""),
    enabled: open && collection?.kind === "smart",
    staleTime: 120_000,
  });

  useEffect(() => {
    if (!open) return;
    const fallbackKind = seed?.kind ?? "smart";
    setName(collection?.name ?? seed?.name ?? "");
    setDescription(collection?.description ?? seed?.description ?? "");
    setColor(collection?.color ?? seed?.color ?? "#5CAAC1");
    setIcon(
      collection?.icon ??
        seed?.icon ??
        ((collection?.kind ?? fallbackKind) === "manual" ? "folder" : "sparkles"),
    );
    setKind(collection?.kind ?? fallbackKind);
    setMatchMode(collection?.matchMode ?? seed?.matchMode ?? "all");
    setRules(collection ? [] : seed ? seedRules(seed) : [defaultRule()]);
    setError(undefined);
  }, [collection, open, seed]);

  useEffect(() => {
    if (open && collection?.kind === "smart" && rulesQuery.data) setRules(rulesQuery.data);
  }, [collection, open, rulesQuery.data]);

  const input = useMemo<SaveCollectionInput>(
    () => ({
      ...(collection ? { id: collection.id } : {}),
      name: name.trim(),
      description: description.trim(),
      color,
      icon,
      kind,
      matchMode,
      rules: kind === "smart" ? rules : [],
    }),
    [collection, color, description, icon, kind, matchMode, name, rules],
  );
  const preview = useMutation({
    mutationFn: () => api.previewSmartCollection(input),
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const save = useMutation({
    mutationFn: () => api.saveCollection(input),
    onSuccess: () => {
      void queryClient.invalidateQueries();
      onOpenChange(false);
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const updateRule = (index: number, update: Partial<SmartRule>) =>
    setRules((current) =>
      current.map((rule, position) => (position === index ? { ...rule, ...update } : rule)),
    );
  const addRule = () =>
    setRules((current) => [
      ...current,
      {
        id: crypto.randomUUID(),
        groupId: 0,
        field: "progress",
        operator: "greaterOrEqual",
        value: 75,
        position: current.length,
      },
    ]);
  const loadingExistingRules =
    editing &&
    kind === "smart" &&
    (rulesQuery.isPending || (rulesQuery.isFetching && !rulesQuery.data));
  const valid =
    name.trim().length >= 2 &&
    (kind === "manual" || (rules.length > 0 && rules.every(ruleHasValidValue))) &&
    !loadingExistingRules &&
    !rulesQuery.isError;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="collection-editor">
        <DialogHeader>
          <DialogTitle>{editing ? "Editar colección" : "Nueva colección"}</DialogTitle>
          <DialogDescription>
            {editing
              ? "Actualiza su identidad y reglas sin perder juegos ni orden manual."
              : "Agrupa juegos manualmente o define reglas que se actualizan solas."}
          </DialogDescription>
        </DialogHeader>
        <div className="collection-editor__fields">
          <label htmlFor="collection-name">
            <span>Nombre</span>
            <Input
              id="collection-name"
              value={name}
              onChange={(event) => setName(event.currentTarget.value)}
              maxLength={80}
              placeholder="Ej. Sesiones de fin de semana"
            />
          </label>
          <label htmlFor="collection-description">
            <span>Descripción</span>
            <Textarea
              id="collection-description"
              rows={2}
              value={description}
              onChange={(event) => setDescription(event.currentTarget.value)}
              maxLength={500}
              placeholder="Qué reúne esta colección"
            />
          </label>
          <div className="collection-editor__identity">
            <label htmlFor="collection-color">
              <span>Color</span>
              <span className="collection-color-field">
                <input
                  id="collection-color"
                  type="color"
                  aria-label="Color de la colección"
                  value={color}
                  onChange={(event) => setColor(event.currentTarget.value)}
                />
                <Input
                  aria-label="Código de color de la colección"
                  value={color}
                  maxLength={7}
                  onChange={(event) => setColor(event.currentTarget.value)}
                />
              </span>
            </label>
            <div className="collection-icon-field">
              <span id="collection-icon-label">Icono</span>
              <CollectionIconPicker
                id="collection-icon"
                label="Icono de la colección"
                value={icon}
                color={color}
                onChange={setIcon}
              />
            </div>
          </div>
        </div>
        {editing ? (
          <div className="collection-kind-lock">
            <CollectionIcon name={icon} fallback={kind} />
            <span>
              Tipo fijo: <strong>{kind === "smart" ? "Inteligente" : "Manual"}</strong>
            </span>
          </div>
        ) : (
          <fieldset className="segmented-control">
            <legend className="sr-only">Tipo de colección</legend>
            <button
              type="button"
              data-active={kind === "manual"}
              onClick={() => {
                setKind("manual");
                setIcon("folder");
              }}
            >
              Manual
            </button>
            <button
              type="button"
              data-active={kind === "smart"}
              onClick={() => {
                setKind("smart");
                setIcon("sparkles");
                setRules((current) => (current.length ? current : [defaultRule()]));
              }}
            >
              <IconSparkles /> Inteligente
            </button>
          </fieldset>
        )}
        {kind === "smart" && (
          <div className="rules-builder" aria-busy={loadingExistingRules}>
            <div className="rules-builder__header">
              <div>
                <strong>Coincidencia de reglas</strong>
                <span>
                  Las condiciones de cada grupo se combinan con Y. Incluye juegos que cumplan
                  {matchMode === "all" ? " todos los grupos" : " cualquier grupo"}.
                </span>
              </div>
              <Select
                value={matchMode}
                onValueChange={(value: "all" | "any") => setMatchMode(value)}
              >
                <SelectTrigger aria-label="Tipo de coincidencia">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">Todas (AND)</SelectItem>
                  <SelectItem value="any">Cualquiera (OR)</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {rulesQuery.isError ? (
              <div className="collection-rules-error" role="alert">
                <span>{getErrorMessage(rulesQuery.error)}</span>
                <Button variant="outline" size="sm" onClick={() => void rulesQuery.refetch()}>
                  Reintentar cargar reglas
                </Button>
              </div>
            ) : loadingExistingRules ? (
              <p className="collection-rules-loading" role="status">
                <IconLoader2 className="is-spinning" /> Cargando reglas guardadas…
              </p>
            ) : (
              <>
                <div className="rule-list">
                  {rules.map((rule, index) => (
                    <RuleRow
                      key={rule.id ?? `${rule.field}-${index}`}
                      rule={rule}
                      statuses={statuses}
                      onChange={(update) => updateRule(index, update)}
                      onRemove={() =>
                        setRules((current) => current.filter((_, position) => position !== index))
                      }
                    />
                  ))}
                </div>
                <Button variant="ghost" size="sm" onClick={addRule}>
                  <IconCirclePlus /> Añadir condición
                </Button>
              </>
            )}
            <div className="preview-panel">
              <div className="preview-panel__header">
                <div>
                  <strong>Vista previa</strong>
                  <span>
                    {preview.data
                      ? `${preview.data.total} juegos coinciden`
                      : "Comprueba las reglas antes de guardar"}
                  </span>
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={!valid || preview.isPending}
                  onClick={() => {
                    setError(undefined);
                    preview.mutate();
                  }}
                >
                  {preview.isPending ? <IconLoader2 className="is-spinning" /> : <IconSparkles />}{" "}
                  Calcular
                </Button>
              </div>
              {preview.data && (
                <div className="preview-games">
                  {preview.data.items.slice(0, 6).map((game) => (
                    <div key={game.appId}>
                      <Artwork
                        appId={game.appId}
                        src={game.iconUrl ?? game.coverUrl}
                        title={game.title}
                        kind="icon"
                      />
                      <span>{game.title}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        {error && (
          <p className="field-error" role="alert">
            {error}
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancelar
          </Button>
          <Button disabled={!valid || save.isPending} onClick={() => save.mutate()}>
            {save.isPending ? (
              <IconLoader2 className="is-spinning" />
            ) : editing ? null : (
              <IconPlus />
            )}
            {editing ? "Guardar cambios" : "Crear colección"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function RuleRow({
  rule,
  statuses,
  onChange,
  onRemove,
}: {
  rule: SmartRule;
  statuses: StatusDefinition[];
  onChange: (update: Partial<SmartRule>) => void;
  onRemove: () => void;
}) {
  const field = fieldOptions.find((option) => option.value === rule.field) ?? installedFieldOption;
  const needsValue = !operatorsWithoutValue.has(rule.operator);
  const multipleValues = rule.operator === "in";
  const displayValue = Array.isArray(rule.value)
    ? rule.value.filter((value) => typeof value === "string").join(", ")
    : String(rule.value ?? "");
  const statusListId = `collection-rule-statuses-${rule.id ?? `${rule.groupId}-${rule.position}`}`;
  const groupOptions = Array.from({ length: Math.max(6, rule.groupId + 1) }, (_, index) => index);
  const setTextValue = (value: string) =>
    onChange({
      value: multipleValues
        ? value
            .split(",")
            .map((item) => item.trim())
            .filter(Boolean)
        : value,
    });

  return (
    <div className="rule-row" data-invalid={!ruleHasValidValue(rule)}>
      <Select
        value={String(rule.groupId)}
        onValueChange={(groupId) => onChange({ groupId: Number(groupId) })}
      >
        <SelectTrigger aria-label="Grupo de la regla">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {groupOptions.map((groupId) => (
            <SelectItem key={groupId} value={String(groupId)}>
              Grupo {groupId + 1}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select
        value={rule.field}
        onValueChange={(nextField) => {
          const next =
            fieldOptions.find((option) => option.value === nextField) ?? installedFieldOption;
          onChange({
            field: next.value,
            operator: next.defaultOperator,
            value: next.defaultValue,
          });
        }}
      >
        <SelectTrigger aria-label="Campo de la regla">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {fieldOptions.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select
        value={rule.operator}
        onValueChange={(operator) =>
          onChange({
            operator,
            value: operatorsWithoutValue.has(operator)
              ? null
              : operatorsWithoutValue.has(rule.operator)
                ? field.defaultValue
                : rule.value,
          })
        }
      >
        <SelectTrigger aria-label="Operador de la regla">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {field.operators.map((operator) => (
            <SelectItem key={operator} value={operator}>
              {operatorLabels[operator]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {!needsValue ? (
        <div className="rule-row__no-value">Sin valor necesario</div>
      ) : field.kind === "boolean" ? (
        <Select
          value={String(rule.value)}
          onValueChange={(value) => onChange({ value: value === "true" })}
        >
          <SelectTrigger aria-label="Valor de la regla">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="true">Sí</SelectItem>
            <SelectItem value="false">No</SelectItem>
          </SelectContent>
        </Select>
      ) : field.kind === "status" && !multipleValues && statuses.length ? (
        <Select value={displayValue} onValueChange={(value) => onChange({ value })}>
          <SelectTrigger aria-label="Estado de la regla">
            <SelectValue placeholder="Selecciona un estado" />
          </SelectTrigger>
          <SelectContent>
            {statuses.map((status) => (
              <SelectItem key={status.id} value={status.id}>
                <span className="rule-status-option">
                  <span style={{ backgroundColor: status.color }} /> {status.name}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <Input
          type={field.kind === "date" ? "date" : field.kind === "number" ? "number" : "text"}
          aria-label="Valor de la regla"
          aria-invalid={!ruleHasValidValue(rule)}
          list={field.kind === "status" ? statusListId : undefined}
          placeholder={multipleValues ? "Separa valores con comas" : undefined}
          value={displayValue}
          onChange={(event) => {
            const value = event.currentTarget.value;
            if (field.kind === "number") {
              onChange({ value: value === "" ? null : Number(value) });
              return;
            }
            setTextValue(value);
          }}
        />
      )}
      <Button variant="ghost" size="icon-sm" aria-label="Eliminar regla" onClick={onRemove}>
        <IconTrash />
      </Button>
      {field.kind === "status" && (
        <datalist id={statusListId}>
          {statuses.map((status) => (
            <option key={status.id} value={status.id}>
              {status.name}
            </option>
          ))}
        </datalist>
      )}
    </div>
  );
}
