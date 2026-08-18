import {
  IconAlarm,
  IconBellPlus,
  IconCalendarRepeat,
  IconLoader2,
  IconPencil,
  IconRefresh,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { AnimatedNumber, RevealOnScroll } from "@/components/motion";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { api, getErrorMessage } from "@/lib/tauri";
import type { NotificationRule } from "@/lib/types";
import { NotificationRuleDialog } from "./NotificationRuleDialog";
import "./notifications.css";
import {
  describeNextOccurrence,
  formatRuleMoment,
  leadLabel,
  repeatLabel,
  ruleKind,
} from "./rule-form";

type RulesFilter = "all" | "enabled" | "paused";

const FILTERS: { id: RulesFilter; label: string }[] = [
  { id: "all", label: "Todos" },
  { id: "enabled", label: "Activos" },
  { id: "paused", label: "Pausados" },
];

/**
 * Panel de reglas de aviso: listar, crear, editar, pausar y borrar.
 *
 * Vive en la pantalla de Seguimiento y no en la bandeja de la barra superior.
 * La bandeja responde a «¿qué ha pasado?» y se cierra al pulsar fuera; este
 * panel responde a «¿qué le he pedido a Vindexa que me diga?», que es una lista
 * que se revisa y se corrige con calma. La creación sí es común a las dos
 * superficies: el mismo diálogo se abre desde aquí y desde la bandeja.
 */
export function NotificationRulesPanel() {
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<RulesFilter>("all");
  const [editing, setEditing] = useState<NotificationRule | undefined>(undefined);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");

  const rules = useQuery({
    queryKey: ["notification-rules"],
    queryFn: () => api.listNotificationRules(),
  });

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: ["notification-rules"] });
    await queryClient.invalidateQueries({ queryKey: ["notification-inbox"] });
  };

  const toggle = useMutation({
    mutationFn: (rule: NotificationRule) =>
      api.saveNotificationRule({
        id: rule.id,
        ...(rule.appId ? { appId: rule.appId } : {}),
        kind: rule.kind,
        title: rule.title,
        body: rule.body,
        ...(rule.scheduledFor ? { scheduledFor: rule.scheduledFor } : {}),
        repeatRule: rule.repeatRule,
        leadMinutes: rule.leadMinutes,
        enabled: !rule.enabled,
      }),
    onSuccess: async (saved) => {
      setAnnouncement(
        saved.enabled
          ? `«${saved.title}» vuelve a estar activo.`
          : `«${saved.title}» queda pausado; la regla y sus fechas se conservan.`,
      );
      await invalidate();
    },
    onError: (cause) => setAnnouncement(`No se pudo cambiar el aviso: ${getErrorMessage(cause)}`),
  });

  const remove = useMutation({
    mutationFn: (id: string) => api.deleteNotificationRule(id),
    onSuccess: async () => {
      setConfirmingDelete(null);
      setAnnouncement("Aviso borrado.");
      await invalidate();
    },
    onError: (cause) => setAnnouncement(`No se pudo borrar: ${getErrorMessage(cause)}`),
  });

  const items = rules.data ?? [];
  const enabledCount = items.filter((rule) => rule.enabled).length;
  const visible = useMemo(() => {
    if (filter === "enabled") return items.filter((rule) => rule.enabled);
    if (filter === "paused") return items.filter((rule) => !rule.enabled);
    return items;
  }, [filter, items]);

  /** La cita más próxima entre las reglas activas. Nunca el ancla. */
  const nextUp = useMemo(() => {
    const dates = items
      .filter((rule) => rule.enabled && rule.nextOccurrence)
      .map((rule) => rule.nextOccurrence as string)
      .sort();
    return dates[0];
  }, [items]);

  const openCreate = () => {
    setEditing(undefined);
    setDialogOpen(true);
  };
  const openEdit = (rule: NotificationRule) => {
    setEditing(rule);
    setDialogOpen(true);
  };

  return (
    <section className="rules-panel" aria-labelledby="rules-panel-title">
      <span className="sr-only" role="status" aria-live="polite">
        {announcement}
      </span>

      <header className="rules-panel__head">
        <IconAlarm aria-hidden="true" />
        <div className="rules-panel__identity">
          <h2 id="rules-panel-title">Avisos programados</h2>
          <p aria-live="polite">
            {rules.isPending
              ? "Leyendo tus avisos"
              : rules.isError
                ? "No se pudieron leer"
                : items.length === 0
                  ? "Ninguno todavía"
                  : nextUp
                    ? `El próximo, el ${formatRuleMoment(nextUp)}`
                    : "Ninguno con cita futura"}
          </p>
        </div>
        <Button size="xs" onClick={openCreate}>
          <IconBellPlus /> Programar
        </Button>
      </header>

      {items.length > 0 && (
        <div className="rules-panel__toolbar">
          <fieldset className="rules-filter">
            <legend className="sr-only">Filtrar avisos programados</legend>
            {FILTERS.map((entry) => (
              <button
                key={entry.id}
                type="button"
                data-active={filter === entry.id}
                aria-pressed={filter === entry.id}
                onClick={() => setFilter(entry.id)}
              >
                {entry.label}
              </button>
            ))}
          </fieldset>
          <p className="rules-panel__count">
            <AnimatedNumber value={enabledCount} /> de {items.length} activos
          </p>
        </div>
      )}

      {rules.isPending ? (
        <p className="rules-panel__note" role="status">
          Leyendo los avisos guardados en tu equipo…
        </p>
      ) : rules.isError ? (
        <div className="rules-panel__error" role="alert">
          <span>{getErrorMessage(rules.error)}</span>
          <Button
            size="xs"
            variant="outline"
            aria-label="Reintentar la lectura de avisos"
            onClick={() => void rules.refetch()}
          >
            <IconRefresh /> Reintentar
          </Button>
        </div>
      ) : items.length === 0 ? (
        <div className="rules-empty">
          <IconAlarm aria-hidden="true" />
          <strong>Todavía no le has pedido nada a Vindexa</strong>
          <p>
            Un aviso programado es una cita tuya: «el 4 de noviembre sale del acceso anticipado»,
            «cada lunes, repasa lo pendiente». Se guarda en tu equipo, se dispara aunque no estés
            mirando y aparece en la bandeja de la barra superior.
          </p>
          <Button size="sm" onClick={openCreate}>
            <IconBellPlus /> Programar el primero
          </Button>
        </div>
      ) : visible.length === 0 ? (
        <p className="rules-panel__note">
          {filter === "enabled"
            ? "No hay ningún aviso activo: todos están pausados."
            : "No hay ningún aviso pausado."}
        </p>
      ) : (
        <ul className="rules-list">
          {visible.map((rule, index) => (
            <RuleRow
              key={rule.id}
              rule={rule}
              index={index}
              busy={toggle.isPending || remove.isPending}
              confirming={confirmingDelete === rule.id}
              onToggle={() => toggle.mutate(rule)}
              onEdit={() => openEdit(rule)}
              onAskDelete={() => setConfirmingDelete(rule.id)}
              onCancelDelete={() => setConfirmingDelete(null)}
              onConfirmDelete={() => remove.mutate(rule.id)}
            />
          ))}
        </ul>
      )}

      <NotificationRuleDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        rule={editing}
        onSaved={(saved) =>
          setAnnouncement(
            `Aviso «${saved.title}» guardado. ${describeNextOccurrence(saved).detail}`,
          )
        }
      />
    </section>
  );
}

function RuleRow({
  rule,
  index,
  busy,
  confirming,
  onToggle,
  onEdit,
  onAskDelete,
  onCancelDelete,
  onConfirmDelete,
}: {
  rule: NotificationRule;
  index: number;
  busy: boolean;
  confirming: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onAskDelete: () => void;
  onCancelDelete: () => void;
  onConfirmDelete: () => void;
}) {
  const next = describeNextOccurrence(rule);
  const kind = ruleKind(rule.kind);
  return (
    // `asChild`: la aparición se aplica al propio `li`, sin añadir un nodo
    // intermedio que rompería la relación `ul > li`.
    <RevealOnScroll asChild delayMs={Math.min(index * 24, 160)}>
      <li className="notification-rule" data-enabled={rule.enabled}>
        <div className="notification-rule__main">
          <p className="notification-rule__title">{rule.title}</p>
          <p className="notification-rule__meta">
            {kind.label}
            {rule.gameTitle ? ` · ${rule.gameTitle}` : ""}
          </p>
          {/* Siempre `nextOccurrence`, nunca el ancla: en una regla mensual del
              día 31 el ancla diría «31» aunque la cita de febrero caiga el 28. */}
          <p className="notification-rule__next" data-state={next.state}>
            <IconCalendarRepeat aria-hidden="true" />
            <span className="sr-only">{next.detail}</span>
            <span aria-hidden="true">
              {next.state === "scheduled" ? `Próximo aviso: ${next.label}` : next.label}
            </span>
          </p>
          <p className="notification-rule__tags">
            <span>{repeatLabel(rule.repeatRule)}</span>
            <span>{leadLabel(rule.leadMinutes)}</span>
          </p>
        </div>

        <div className="notification-rule__actions">
          <Switch
            size="sm"
            checked={rule.enabled}
            disabled={busy}
            aria-label={rule.enabled ? `Pausar «${rule.title}»` : `Activar «${rule.title}»`}
            onCheckedChange={onToggle}
          />
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label={`Editar «${rule.title}»`}
            onClick={onEdit}
          >
            <IconPencil />
          </Button>
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label={`Borrar «${rule.title}»`}
            disabled={busy}
            onClick={onAskDelete}
          >
            <IconTrash />
          </Button>
        </div>

        {confirming && (
          <div className="notification-rule__confirm">
            <span>¿Borrar «{rule.title}»? No se puede deshacer.</span>
            <Button size="xs" variant="destructive" disabled={busy} onClick={onConfirmDelete}>
              {busy ? <IconLoader2 className="is-spinning" /> : <IconTrash />} Borrar
            </Button>
            <Button size="xs" variant="ghost" onClick={onCancelDelete}>
              Conservar
            </Button>
          </div>
        )}
      </li>
    </RevealOnScroll>
  );
}
