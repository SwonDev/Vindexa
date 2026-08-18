import { IconAlertTriangle, IconLoader2, IconSearch } from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useDeferredValue, useEffect, useId, useMemo, useState } from "react";
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
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, getErrorMessage } from "@/lib/tauri";
import type { NotificationRule } from "@/lib/types";
import "./notifications.css";
import {
  emptyRuleForm,
  formToInput,
  LEAD_CHOICES,
  MAX_RULE_TITLE,
  pastDateWarning,
  REPEAT_RULES,
  RULE_KINDS,
  type RuleFormErrors,
  type RuleFormState,
  ruleKind,
  ruleToForm,
  validateRuleForm,
} from "./rule-form";

export interface RulePrefill {
  title?: string;
  body?: string;
  /** Valor de `datetime-local`; ya en hora local. */
  scheduledForLocal?: string;
  appId?: number;
  gameTitle?: string;
}

export interface NotificationRuleDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Regla existente. Si falta, el diálogo crea una nueva. */
  rule?: NotificationRule | undefined;
  /** Valores de partida al crear. Se ignoran al editar. */
  prefill?: RulePrefill | undefined;
  onSaved?: ((rule: NotificationRule) => void) | undefined;
}

function initialState(
  rule: NotificationRule | undefined,
  prefill: RulePrefill | undefined,
): RuleFormState {
  if (rule) return ruleToForm(rule);
  const base = emptyRuleForm();
  return {
    ...base,
    title: prefill?.title ?? base.title,
    body: prefill?.body ?? base.body,
    scheduledForLocal: prefill?.scheduledForLocal ?? base.scheduledForLocal,
    appId: prefill?.appId ?? base.appId,
    gameTitle: prefill?.gameTitle ?? base.gameTitle,
  };
}

/**
 * Editor de una regla de aviso.
 *
 * Vive en un diálogo modal, y no dentro de la bandeja, porque programar un
 * aviso es una tarea deliberada: hay tipo, fecha, repetición y margen que
 * elegir, y un popover se cierra al pulsar fuera o con `Escape`, que es
 * exactamente lo que no debe pasarle a un formulario a medio escribir.
 *
 * Se abre desde los dos sitios donde surge la intención: el panel de avisos
 * programados de Seguimiento y la bandeja de la barra superior.
 */
export function NotificationRuleDialog({
  open,
  onOpenChange,
  rule,
  prefill,
  onSaved,
}: NotificationRuleDialogProps) {
  const queryClient = useQueryClient();
  const fieldId = useId();
  const [form, setForm] = useState<RuleFormState>(() => initialState(rule, prefill));
  const [showErrors, setShowErrors] = useState(false);
  const [gameSearch, setGameSearch] = useState("");
  const deferredSearch = useDeferredValue(gameSearch);

  // Cada apertura parte del estado que corresponde: la regla que se edita o los
  // valores de partida. Sin esto, editar una regla después de otra arrastraría
  // los datos de la anterior.
  useEffect(() => {
    if (!open) return;
    setForm(initialState(rule, prefill));
    setShowErrors(false);
    setGameSearch("");
  }, [open, rule, prefill]);

  const kind = ruleKind(form.kind);
  const errors: RuleFormErrors = useMemo(() => validateRuleForm(form), [form]);
  const warning = useMemo(() => pastDateWarning(form), [form]);
  const visibleErrors = showErrors ? errors : {};

  const search = deferredSearch.trim();
  const games = useQuery({
    queryKey: ["notification-rule-games", search],
    queryFn: () => api.listGames({ query: search, limit: 8, offset: 0, sort: "alphabetical" }),
    enabled: open && search.length >= 2,
    staleTime: 30_000,
  });

  const save = useMutation({
    mutationFn: () => api.saveNotificationRule(formToInput(form)),
    onSuccess: async (saved) => {
      await queryClient.invalidateQueries({ queryKey: ["notification-rules"] });
      await queryClient.invalidateQueries({ queryKey: ["notification-inbox"] });
      onSaved?.(saved);
      onOpenChange(false);
    },
  });

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setShowErrors(true);
    if (Object.keys(errors).length > 0) return;
    save.mutate();
  };

  const titleLength = [...form.title].length;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="rule-dialog">
        <DialogHeader>
          <DialogTitle>{rule ? "Editar aviso programado" : "Programar un aviso"}</DialogTitle>
          <DialogDescription>
            El aviso se guarda en tu equipo y aparece en la bandeja cuando llega su cita. Vindexa no
            envía nada a ningún servidor.
          </DialogDescription>
        </DialogHeader>

        <form className="rule-form" onSubmit={submit} noValidate>
          <div className="rule-form__row">
            <div className="rule-field">
              <span className="rule-field__label" id={`${fieldId}-kind-label`}>
                Tipo de aviso
              </span>
              <Select
                value={form.kind}
                onValueChange={(value) =>
                  setForm((current) => ({
                    ...current,
                    kind: value as RuleFormState["kind"],
                  }))
                }
              >
                <SelectTrigger
                  id={`${fieldId}-kind`}
                  aria-labelledby={`${fieldId}-kind-label`}
                  aria-label="Tipo de aviso"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {RULE_KINDS.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="rule-field__hint">{kind.hint}</span>
            </div>

            <label className="rule-field" htmlFor={`${fieldId}-date`}>
              <span className="rule-field__label">
                Primera cita
                <em>ancla de la repetición</em>
              </span>
              <Input
                id={`${fieldId}-date`}
                type="datetime-local"
                value={form.scheduledForLocal}
                aria-invalid={Boolean(visibleErrors.scheduledForLocal)}
                aria-describedby={`${fieldId}-date-help`}
                onChange={(event) =>
                  setForm((current) => ({ ...current, scheduledForLocal: event.target.value }))
                }
              />
              <span className="rule-field__hint" id={`${fieldId}-date-help`}>
                {visibleErrors.scheduledForLocal ? (
                  <strong className="rule-field__error">{visibleErrors.scheduledForLocal}</strong>
                ) : (
                  "Esta fecha no se reescribe: las repeticiones se cuentan desde ella."
                )}
              </span>
            </label>
          </div>

          <label className="rule-field" htmlFor={`${fieldId}-title`}>
            <span className="rule-field__label">
              Título
              <em>
                {titleLength}/{MAX_RULE_TITLE}
              </em>
            </span>
            <Input
              id={`${fieldId}-title`}
              value={form.title}
              placeholder="Sale del acceso anticipado"
              aria-invalid={Boolean(visibleErrors.title)}
              aria-describedby={`${fieldId}-title-help`}
              onChange={(event) =>
                setForm((current) => ({ ...current, title: event.target.value }))
              }
            />
            <span className="rule-field__hint" id={`${fieldId}-title-help`}>
              {visibleErrors.title ? (
                <strong className="rule-field__error">{visibleErrors.title}</strong>
              ) : (
                "Es lo que leerás en la bandeja: escríbelo como te lo dirías a ti."
              )}
            </span>
          </label>

          <label className="rule-field" htmlFor={`${fieldId}-body`}>
            <span className="rule-field__label">
              Detalle
              <em>opcional</em>
            </span>
            <Textarea
              id={`${fieldId}-body`}
              rows={2}
              value={form.body}
              aria-invalid={Boolean(visibleErrors.body)}
              onChange={(event) => setForm((current) => ({ ...current, body: event.target.value }))}
            />
            {visibleErrors.body && (
              <strong className="rule-field__error">{visibleErrors.body}</strong>
            )}
          </label>

          <fieldset className="rule-field rule-field--game">
            <legend className="rule-field__label">
              Juego
              <em>{kind.requiresGame ? "obligatorio para este tipo" : "opcional"}</em>
            </legend>
            <div className="rule-game__current">
              <span>{form.appId ? form.gameTitle || `AppID ${form.appId}` : "Sin juego"}</span>
              {form.appId ? (
                <Button
                  type="button"
                  size="xs"
                  variant="ghost"
                  onClick={() => setForm((current) => ({ ...current, appId: null, gameTitle: "" }))}
                >
                  Quitar
                </Button>
              ) : null}
            </div>
            <div className="rule-game__search">
              <IconSearch aria-hidden="true" />
              <Input
                type="search"
                value={gameSearch}
                placeholder="Buscar en tu biblioteca…"
                aria-label="Buscar un juego de tu biblioteca"
                onChange={(event) => setGameSearch(event.target.value)}
              />
            </div>
            {search.length >= 2 ? (
              games.isPending ? (
                <p className="rule-game__note" role="status">
                  Buscando en tu biblioteca…
                </p>
              ) : games.isError ? (
                <p className="rule-game__note" role="alert">
                  {getErrorMessage(games.error)}
                </p>
              ) : games.data?.items.length ? (
                <ul className="rule-game__results">
                  {games.data.items.map((game) => (
                    <li key={game.appId}>
                      <button
                        type="button"
                        data-selected={form.appId === game.appId}
                        onClick={() =>
                          setForm((current) => ({
                            ...current,
                            appId: game.appId,
                            gameTitle: game.title,
                          }))
                        }
                      >
                        {game.title}
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="rule-game__note">
                  Ningún juego de tu biblioteca coincide con «{search}».
                </p>
              )
            ) : (
              <p className="rule-game__note">
                Escribe al menos dos letras para buscar. Solo aparecen juegos ya importados.
              </p>
            )}
            {visibleErrors.appId && (
              <strong className="rule-field__error">{visibleErrors.appId}</strong>
            )}
          </fieldset>

          <div className="rule-form__row">
            <div className="rule-field">
              <span className="rule-field__label" id={`${fieldId}-repeat-label`}>
                Repetición
              </span>
              <Select
                value={form.repeatRule}
                onValueChange={(value) =>
                  setForm((current) => ({
                    ...current,
                    repeatRule: value as RuleFormState["repeatRule"],
                  }))
                }
              >
                <SelectTrigger
                  id={`${fieldId}-repeat`}
                  aria-labelledby={`${fieldId}-repeat-label`}
                  aria-label="Repetición"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {REPEAT_RULES.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="rule-field__hint">
                {form.repeatRule === "monthly"
                  ? "Se conserva el día que elijas: un aviso del 31 cae el 28 en febrero y vuelve al 31 en marzo."
                  : "Cada repetición se cuenta desde la primera cita."}
              </span>
            </div>

            <div className="rule-field">
              <span className="rule-field__label" id={`${fieldId}-lead-label`}>
                Margen
              </span>
              <Select
                value={String(form.leadMinutes)}
                onValueChange={(value) =>
                  setForm((current) => ({ ...current, leadMinutes: Number(value) }))
                }
              >
                <SelectTrigger
                  id={`${fieldId}-lead`}
                  aria-labelledby={`${fieldId}-lead-label`}
                  aria-label="Margen de aviso"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {LEAD_CHOICES.map((option) => (
                    <SelectItem key={option.value} value={String(option.value)}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="rule-field__hint">
                {visibleErrors.leadMinutes ? (
                  <strong className="rule-field__error">{visibleErrors.leadMinutes}</strong>
                ) : (
                  "Cuánto antes de la cita quieres verlo en la bandeja."
                )}
              </span>
            </div>
          </div>

          <div className="rule-form__enabled">
            <Switch
              id={`${fieldId}-enabled`}
              checked={form.enabled}
              onCheckedChange={(checked) =>
                setForm((current) => ({ ...current, enabled: checked === true }))
              }
            />
            <label htmlFor={`${fieldId}-enabled`}>
              Activado
              <span>Pausarlo conserva la regla y sus fechas; simplemente deja de enviarse.</span>
            </label>
          </div>

          {warning && (
            <p className="rule-form__warning">
              <IconAlertTriangle aria-hidden="true" />
              {warning}
            </p>
          )}

          {save.isError && (
            <p className="rule-form__error" role="alert">
              <IconAlertTriangle aria-hidden="true" />
              {getErrorMessage(save.error)}
            </p>
          )}

          <DialogFooter>
            <Button type="button" variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              Cancelar
            </Button>
            <Button type="submit" size="sm" disabled={save.isPending}>
              {save.isPending ? <IconLoader2 className="is-spinning" /> : null}
              {rule ? "Guardar cambios" : "Programar aviso"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
