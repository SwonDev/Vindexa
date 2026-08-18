import {
  IconAlarm,
  IconAlertTriangle,
  IconArchive,
  IconBell,
  IconBellPlus,
  IconCheck,
  IconChecks,
  IconCircleCheck,
  IconInfoCircle,
  IconLoader2,
  IconRefresh,
  IconX,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Overlay } from "@/components/common/Overlay";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatRelativeDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { NotificationEvent, NotificationInboxScope, NotificationSeverity } from "@/lib/types";
import { NotificationRuleDialog } from "./NotificationRuleDialog";
import "./notifications.css";
import { formatRuleMoment } from "./rule-form";

const SCOPES: { id: NotificationInboxScope; label: string; hint: string }[] = [
  { id: "pending", label: "Pendientes", hint: "Avisos sin descartar" },
  { id: "unread", label: "Sin leer", hint: "Solo los que no has abierto" },
  { id: "all", label: "Todos", hint: "Incluye los descartados" },
];

const SEVERITY_ICON: Record<NotificationSeverity, typeof IconInfoCircle> = {
  info: IconInfoCircle,
  success: IconCircleCheck,
  warning: IconAlertTriangle,
  critical: IconAlertTriangle,
};

const SEVERITY_LABEL: Record<NotificationSeverity, string> = {
  info: "Información",
  success: "Buena noticia",
  warning: "Atención",
  critical: "Urgente",
};

/**
 * Bandeja de avisos de la barra superior.
 *
 * Muestra tanto las reglas que la persona programa como los eventos derivados
 * de señales oficiales que Vindexa ya observa. Nada aquí se inventa: cada
 * entrada procede de un hecho verificable guardado en la base local.
 */
export function NotificationsPopover() {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<NotificationInboxScope>("pending");
  const [composing, setComposing] = useState(false);

  const inbox = useQuery({
    queryKey: ["notification-inbox", scope],
    queryFn: () => api.notificationInbox({ scope }, 40, 0),
    refetchInterval: open ? 30_000 : 120_000,
  });
  const unread = inbox.data?.unread;
  const events = inbox.data?.items ?? [];

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["notification-inbox"] });

  const refresh = useMutation({
    mutationFn: api.refreshNotificationEvents,
    onSuccess: () => void invalidate(),
  });
  const markRead = useMutation({
    mutationFn: (id: string) => api.markNotificationRead(id),
    onSuccess: () => void invalidate(),
  });
  const markAllRead = useMutation({
    mutationFn: api.markAllNotificationsRead,
    onSuccess: () => void invalidate(),
  });
  const dismissAll = useMutation({
    mutationFn: () => api.dismissAllNotifications(),
    onSuccess: () => void invalidate(),
  });
  const dismiss = useMutation({
    mutationFn: (id: string) => api.dismissNotification(id),
    onSuccess: () => void invalidate(),
  });

  /**
   * Solo se consulta con la bandeja abierta: el pie es contexto de la bandeja,
   * no un contador permanente de la barra superior.
   */
  const rules = useQuery({
    queryKey: ["notification-rules"],
    queryFn: () => api.listNotificationRules(),
    enabled: open,
    retry: false,
  });
  const activeRules = rules.data?.filter((rule) => rule.enabled) ?? [];
  const nextRule = activeRules
    .map((rule) => rule.nextOccurrence)
    .filter((value): value is string => Boolean(value))
    .sort()[0];

  const pendingCount = unread?.total ?? 0;
  // Descartar vacía los que siguen en la vista, estén leídos o no; marcar como
  // leído sólo afecta a los no leídos. Por eso son dos cuentas distintas.
  const activeCount = events.filter((event) => !event.dismissedAt).length;
  const urgent = (unread?.critical ?? 0) + (unread?.warning ?? 0) > 0;

  return (
    <>
      {/* La bandeja es una capa sobre la biblioteca, no una anotación al
          margen: sin velo se leía flotando sin separación del listado. */}
      <Overlay open={open} />
      <Popover
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          // Al abrir se derivan los eventos oficiales nuevos: la bandeja siempre
          // refleja lo que Vindexa ya ha observado, sin un temporizador aparte.
          if (next && !refresh.isPending) refresh.mutate();
        }}
      >
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="notifications-trigger"
                data-urgent={urgent}
                aria-label={
                  pendingCount ? `Avisos: ${pendingCount} sin leer` : "Avisos: ninguno sin leer"
                }
              >
                <IconBell />
                {pendingCount > 0 && (
                  <span className="notifications-badge" aria-hidden="true">
                    {pendingCount > 99 ? "99+" : pendingCount}
                  </span>
                )}
              </Button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent>
            {pendingCount
              ? `${pendingCount} aviso${pendingCount === 1 ? "" : "s"} sin leer`
              : "Avisos"}
          </TooltipContent>
        </Tooltip>
        <PopoverContent align="end" className="notifications-panel" sideOffset={6}>
          <header className="notifications-panel__head">
            <div>
              <p className="notifications-panel__title">Avisos</p>
              <p className="notifications-panel__meta">
                {inbox.data
                  ? `${inbox.data.total.toLocaleString("es-ES")} en esta vista · ${pendingCount.toLocaleString("es-ES")} sin leer`
                  : "Cargando…"}
              </p>
            </div>
            <div className="notifications-panel__tools">
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="Buscar eventos oficiales nuevos"
                onClick={() => refresh.mutate()}
                disabled={refresh.isPending}
              >
                {refresh.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}
              </Button>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="Marcar todo como leído"
                onClick={() => markAllRead.mutate()}
                disabled={markAllRead.isPending || pendingCount === 0}
              >
                <IconChecks />
              </Button>
              {/* Descartar no es lo mismo que marcar leído: lo primero saca el
                  aviso de la vista de pendientes, lo segundo sólo le quita el
                  resalte. Quien vuelve tras una semana quiere lo primero. */}
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="Descartar todos los avisos"
                onClick={() => dismissAll.mutate()}
                disabled={dismissAll.isPending || activeCount === 0}
              >
                {dismissAll.isPending ? <IconLoader2 className="is-spinning" /> : <IconArchive />}
              </Button>
            </div>
          </header>

          <nav className="notifications-scopes" aria-label="Filtrar avisos">
            {SCOPES.map((entry) => (
              <button
                key={entry.id}
                type="button"
                aria-pressed={scope === entry.id}
                title={entry.hint}
                data-active={scope === entry.id}
                onClick={() => setScope(entry.id)}
              >
                {entry.label}
              </button>
            ))}
          </nav>

          <ul className="notifications-list">
            {inbox.isPending && <li className="notifications-empty">Cargando avisos…</li>}
            {inbox.isError && (
              <li className="notifications-empty" role="alert">
                {getErrorMessage(inbox.error)}
              </li>
            )}
            {!inbox.isPending && !inbox.isError && events.length === 0 && (
              <li className="notifications-empty">
                {scope === "pending"
                  ? "Nada pendiente. Los avisos que programes abajo aparecerán aquí al llegar su cita."
                  : "No hay avisos en esta vista."}
              </li>
            )}
            {events.map((event) => (
              <NotificationRow
                key={event.id}
                event={event}
                onRead={() => markRead.mutate(event.id)}
                onDismiss={() => dismiss.mutate(event.id)}
              />
            ))}
          </ul>

          {/*
          Crear una regla no cabe aquí: hay tipo, fecha, repetición y margen que
          elegir, y este panel se cierra al pulsar fuera o con `Escape`. Lo que
          sí vive aquí es el atajo, porque la intención de programar un aviso
          nace justo al leer lo que ha pasado. El formulario se abre en un
          diálogo modal y la gestión completa está en Seguimiento.
        */}
          <footer className="notifications-panel__foot">
            <p>
              <IconAlarm aria-hidden="true" />
              {rules.isError
                ? "No se pudieron leer tus avisos programados."
                : rules.isPending
                  ? "Leyendo tus avisos programados…"
                  : activeRules.length === 0
                    ? "No tienes ningún aviso programado."
                    : nextRule
                      ? `${activeRules.length} programados · el próximo, el ${formatRuleMoment(nextRule)}`
                      : `${activeRules.length} programados · ninguno con cita futura`}
            </p>
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                setOpen(false);
                setComposing(true);
              }}
            >
              <IconBellPlus /> Programar
            </Button>
          </footer>
        </PopoverContent>
      </Popover>
      <NotificationRuleDialog open={composing} onOpenChange={setComposing} />
    </>
  );
}

function NotificationRow({
  event,
  onRead,
  onDismiss,
}: {
  event: NotificationEvent;
  onRead: () => void;
  onDismiss: () => void;
}) {
  const SeverityIcon = SEVERITY_ICON[event.severity] ?? IconInfoCircle;
  const unread = !event.readAt;
  return (
    <li className="notification-row" data-severity={event.severity} data-unread={unread}>
      <SeverityIcon aria-label={SEVERITY_LABEL[event.severity] ?? "Aviso"} size={15} stroke={1.9} />
      <div className="notification-row__body">
        <p className="notification-row__title">{event.title}</p>
        {event.body && <p className="notification-row__text">{event.body}</p>}
        <p className="notification-row__meta">
          {event.gameTitle ? `${event.gameTitle} · ` : ""}
          {formatRelativeDate(event.occurredAt)}
        </p>
      </div>
      <div className="notification-row__actions">
        {unread && (
          <Button variant="ghost" size="icon-xs" aria-label="Marcar como leído" onClick={onRead}>
            <IconCheck />
          </Button>
        )}
        {!event.dismissedAt && (
          <Button variant="ghost" size="icon-xs" aria-label="Descartar aviso" onClick={onDismiss}>
            <IconX />
          </Button>
        )}
      </div>
    </li>
  );
}
