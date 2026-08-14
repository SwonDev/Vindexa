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
import { CollectionIcon, collectionIconOptions } from "@/features/collections/CollectionIcon";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  CollectionSummary,
  SaveCollectionInput,
  SmartRule,
  StatusDefinition,
} from "@/lib/types";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  collection?: CollectionSummary | undefined;
  statuses?: StatusDefinition[] | undefined;
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

export function CollectionEditorDialog({ open, onOpenChange, collection, statuses = [] }: Props) {
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

  const rulesQuery = useQuery({
    queryKey: ["collection-rules", collection?.id],
    queryFn: () => api.listSmartRules(collection?.id ?? ""),
    enabled: open && collection?.kind === "smart",
  });

  useEffect(() => {
    if (!open) return;
    setName(collection?.name ?? "");
    setDescription(collection?.description ?? "");
    setColor(collection?.color ?? "#5CAAC1");
    setIcon(collection?.icon ?? (collection?.kind === "manual" ? "folder" : "sparkles"));
    setKind(collection?.kind ?? "smart");
    setMatchMode(collection?.matchMode ?? "all");
    setRules(collection ? [] : [defaultRule()]);
    setError(undefined);
  }, [collection, open]);

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
            <label htmlFor="collection-icon">
              <span>Icono</span>
              <Select value={icon} onValueChange={setIcon}>
                <SelectTrigger id="collection-icon" aria-label="Icono de la colección">
                  <CollectionIcon name={icon} fallback={kind} />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {collectionIconOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      <option.icon aria-hidden="true" /> {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
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
