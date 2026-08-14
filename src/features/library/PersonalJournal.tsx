import {
  IconCalendarEvent,
  IconCheck,
  IconEdit,
  IconPlus,
  IconTags,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { formatDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  GameDetail,
  GameSession,
  SavePersonalDatesInput,
  SaveSessionInput,
  SaveTagInput,
  TagDefinition,
} from "@/lib/types";

interface Props {
  detail: GameDetail;
}

interface SessionDraft {
  id?: string;
  startedAt: string;
  endedAt: string;
  progressBefore: string;
  progressAfter: string;
  note: string;
}

const emptySession = (): SessionDraft => ({
  startedAt: toLocalDateTimeInput(new Date().toISOString()),
  endedAt: "",
  progressBefore: "",
  progressAfter: "",
  note: "",
});

export function PersonalJournal({ detail }: Props) {
  const queryClient = useQueryClient();
  const [startedAt, setStartedAt] = useState(detail.startedAt ?? "");
  const [completedAt, setCompletedAt] = useState(detail.completedAt ?? "");
  const [abandonedAt, setAbandonedAt] = useState(detail.abandonedAt ?? "");
  const [tagDraft, setTagDraft] = useState<SaveTagInput>({ name: "", color: "#5CAAC1" });
  const [sessionDraft, setSessionDraft] = useState<SessionDraft>(emptySession);
  const [olderSessions, setOlderSessions] = useState<GameSession[]>([]);

  const tagsQuery = useQuery({ queryKey: ["personal-tags"], queryFn: api.listTags });
  useEffect(() => {
    setStartedAt(detail.startedAt ?? "");
    setCompletedAt(detail.completedAt ?? "");
    setAbandonedAt(detail.abandonedAt ?? "");
  }, [detail.startedAt, detail.completedAt, detail.abandonedAt]);
  useEffect(() => {
    if (detail.appId > 0) {
      setSessionDraft(emptySession());
      setOlderSessions([]);
    }
  }, [detail.appId]);

  const visibleSessions = useMemo(
    () =>
      Array.from(
        new Map(
          [...detail.sessions, ...olderSessions].map((session) => [session.id, session]),
        ).values(),
      ),
    [detail.sessions, olderSessions],
  );
  const sessionsTotal = detail.sessionsTotal ?? detail.sessions.length;

  const storeDetail = (next: GameDetail) => {
    queryClient.setQueryData(["game", detail.appId], next);
    void queryClient.invalidateQueries({ queryKey: ["games"] });
    void queryClient.invalidateQueries({ queryKey: ["library-filter-options"] });
  };
  const datesMutation = useMutation({
    mutationFn: (input: SavePersonalDatesInput) => api.savePersonalDates(input),
    onSuccess: storeDetail,
  });
  const assignmentMutation = useMutation({
    mutationFn: (tagIds: string[]) => api.setGameTags(detail.appId, tagIds),
    onSuccess: storeDetail,
  });
  const tagMutation = useMutation({
    mutationFn: (input: SaveTagInput) => api.saveTag(input),
    onSuccess: async () => {
      setTagDraft({ name: "", color: "#5CAAC1" });
      await queryClient.invalidateQueries({ queryKey: ["personal-tags"] });
      await queryClient.invalidateQueries({ queryKey: ["library-filter-options"] });
    },
  });
  const deleteTagMutation = useMutation({
    mutationFn: api.deleteTag,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["personal-tags"] });
      await queryClient.invalidateQueries({ queryKey: ["game", detail.appId] });
      await queryClient.invalidateQueries({ queryKey: ["library-filter-options"] });
    },
  });
  const sessionMutation = useMutation({
    mutationFn: (input: SaveSessionInput) => api.saveGameSession(input),
    onSuccess: (next) => {
      setSessionDraft(emptySession());
      setOlderSessions([]);
      storeDetail(next);
    },
  });
  const deleteSessionMutation = useMutation({
    mutationFn: api.deleteGameSession,
    onSuccess: (next) => {
      setOlderSessions([]);
      storeDetail(next);
    },
  });
  const loadSessionsMutation = useMutation({
    mutationFn: () => api.listGameSessions(detail.appId, 50, visibleSessions.length),
    onSuccess: (page) => {
      setOlderSessions((current) =>
        Array.from(
          new Map([...current, ...page.items].map((session) => [session.id, session])).values(),
        ),
      );
    },
  });

  const selectedTagIds = detail.tagIds ?? [];
  const submitDates = () => {
    datesMutation.mutate({
      appId: detail.appId,
      ...(startedAt ? { startedAt } : {}),
      ...(completedAt ? { completedAt } : {}),
      ...(abandonedAt ? { abandonedAt } : {}),
    });
  };
  const submitSession = () => {
    if (!sessionDraft.startedAt) return;
    sessionMutation.mutate({
      ...(sessionDraft.id ? { id: sessionDraft.id } : {}),
      appId: detail.appId,
      startedAt: new Date(sessionDraft.startedAt).toISOString(),
      ...(sessionDraft.endedAt ? { endedAt: new Date(sessionDraft.endedAt).toISOString() } : {}),
      ...(sessionDraft.progressBefore
        ? { progressBefore: Number(sessionDraft.progressBefore) }
        : {}),
      ...(sessionDraft.progressAfter ? { progressAfter: Number(sessionDraft.progressAfter) } : {}),
      note: sessionDraft.note,
    });
  };

  return (
    <div className="personal-journal">
      <section className="journal-section" aria-labelledby="personal-dates-title">
        <header>
          <div>
            <span className="journal-section__icon">
              <IconCalendarEvent />
            </span>
            <div>
              <h3 id="personal-dates-title">Fechas personales</h3>
              <p>Tu propia línea temporal, independiente de Steam.</p>
            </div>
          </div>
          <Button size="sm" onClick={submitDates} disabled={datesMutation.isPending}>
            <IconCheck /> Guardar fechas
          </Button>
        </header>
        <div className="journal-date-grid">
          <label htmlFor="personal-started-at">
            <span>Inicio</span>
            <Input
              id="personal-started-at"
              aria-label="Fecha de inicio personal"
              type="date"
              value={startedAt}
              onChange={(event) => setStartedAt(event.target.value)}
            />
          </label>
          <label htmlFor="personal-completed-at">
            <span>Finalización</span>
            <Input
              id="personal-completed-at"
              aria-label="Fecha de finalización personal"
              type="date"
              value={completedAt}
              onChange={(event) => {
                setCompletedAt(event.target.value);
                if (event.target.value) setAbandonedAt("");
              }}
            />
          </label>
          <label htmlFor="personal-abandoned-at">
            <span>Abandono</span>
            <Input
              id="personal-abandoned-at"
              aria-label="Fecha de abandono personal"
              type="date"
              value={abandonedAt}
              onChange={(event) => {
                setAbandonedAt(event.target.value);
                if (event.target.value) setCompletedAt("");
              }}
            />
          </label>
        </div>
        <MutationMessage mutation={datesMutation} success="Fechas personales guardadas." />
      </section>

      <section className="journal-section" aria-labelledby="personal-tags-title">
        <header>
          <div>
            <span className="journal-section__icon">
              <IconTags />
            </span>
            <div>
              <h3 id="personal-tags-title">Etiquetas</h3>
              <p>Clasificación transversal para filtros y colecciones inteligentes.</p>
            </div>
          </div>
        </header>
        <fieldset className="journal-tag-list">
          <legend className="sr-only">Etiquetas disponibles</legend>
          {tagsQuery.isPending ? (
            <p className="muted-copy">Cargando etiquetas…</p>
          ) : tagsQuery.isError ? (
            <div className="journal-query-error" role="alert">
              <p>No se pudieron cargar tus etiquetas.</p>
              <Button size="sm" variant="outline" onClick={() => void tagsQuery.refetch()}>
                Reintentar
              </Button>
            </div>
          ) : tagsQuery.data?.length ? (
            tagsQuery.data.map((tag) => (
              <TagRow
                key={tag.id}
                tag={tag}
                checked={selectedTagIds.includes(tag.id)}
                busy={assignmentMutation.isPending || deleteTagMutation.isPending}
                onToggle={(checked) =>
                  assignmentMutation.mutate(
                    checked
                      ? [...selectedTagIds, tag.id]
                      : selectedTagIds.filter((id) => id !== tag.id),
                  )
                }
                onEdit={() => setTagDraft(tag)}
                onDelete={() => deleteTagMutation.mutate(tag.id)}
              />
            ))
          ) : (
            <p className="muted-copy">Aún no has creado etiquetas personales.</p>
          )}
        </fieldset>
        <div className="journal-tag-editor">
          <Input
            aria-label="Nombre de la etiqueta"
            maxLength={40}
            placeholder="Nueva etiqueta"
            value={tagDraft.name}
            onChange={(event) => setTagDraft({ ...tagDraft, name: event.target.value })}
          />
          <Input
            aria-label="Color de la etiqueta"
            type="color"
            value={tagDraft.color}
            onChange={(event) => setTagDraft({ ...tagDraft, color: event.target.value })}
          />
          <Button
            size="sm"
            disabled={!tagDraft.name.trim() || tagMutation.isPending}
            onClick={() => tagMutation.mutate(tagDraft)}
          >
            {tagDraft.id ? <IconCheck /> : <IconPlus />}
            {tagDraft.id ? "Guardar etiqueta" : "Crear etiqueta"}
          </Button>
          {tagDraft.id && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setTagDraft({ name: "", color: "#5CAAC1" })}
            >
              Cancelar
            </Button>
          )}
        </div>
        <MutationMessage mutation={tagMutation} success="Etiqueta guardada." />
        <MutationMessage mutation={deleteTagMutation} success="Etiqueta eliminada." />
        <MutationMessage mutation={assignmentMutation} success="Asignación actualizada." />
      </section>

      <section className="journal-section" aria-labelledby="personal-sessions-title">
        <header>
          <div>
            <span className="journal-section__icon">
              <IconCalendarEvent />
            </span>
            <div>
              <h3 id="personal-sessions-title">Sesiones de juego</h3>
              <p>Registra cuánto avanzaste y conserva el contexto de cada partida.</p>
            </div>
          </div>
        </header>
        <div className="session-editor">
          <label htmlFor="session-started-at">
            <span>Inicio</span>
            <Input
              id="session-started-at"
              type="datetime-local"
              value={sessionDraft.startedAt}
              onChange={(event) =>
                setSessionDraft({ ...sessionDraft, startedAt: event.target.value })
              }
            />
          </label>
          <label htmlFor="session-ended-at">
            <span>Final</span>
            <Input
              id="session-ended-at"
              type="datetime-local"
              value={sessionDraft.endedAt}
              onChange={(event) =>
                setSessionDraft({ ...sessionDraft, endedAt: event.target.value })
              }
            />
          </label>
          <label htmlFor="session-progress-before">
            <span>Progreso antes</span>
            <Input
              id="session-progress-before"
              type="number"
              min={0}
              max={100}
              placeholder="%"
              value={sessionDraft.progressBefore}
              onChange={(event) =>
                setSessionDraft({ ...sessionDraft, progressBefore: event.target.value })
              }
            />
          </label>
          <label htmlFor="session-progress-after">
            <span>Progreso después</span>
            <Input
              id="session-progress-after"
              type="number"
              min={0}
              max={100}
              placeholder="%"
              value={sessionDraft.progressAfter}
              onChange={(event) =>
                setSessionDraft({ ...sessionDraft, progressAfter: event.target.value })
              }
            />
          </label>
          <label className="session-editor__note" htmlFor="session-note">
            <span>Nota de sesión</span>
            <Textarea
              id="session-note"
              maxLength={2000}
              rows={3}
              placeholder="Qué hiciste, dónde lo dejaste o qué quieres recordar"
              value={sessionDraft.note}
              onChange={(event) => setSessionDraft({ ...sessionDraft, note: event.target.value })}
            />
          </label>
          <div className="session-editor__actions">
            <Button
              size="sm"
              disabled={!sessionDraft.startedAt || sessionMutation.isPending}
              onClick={submitSession}
            >
              {sessionDraft.id ? <IconCheck /> : <IconPlus />}
              {sessionDraft.id ? "Guardar sesión" : "Registrar sesión"}
            </Button>
            {sessionDraft.id && (
              <Button size="sm" variant="ghost" onClick={() => setSessionDraft(emptySession())}>
                Cancelar edición
              </Button>
            )}
          </div>
        </div>
        <MutationMessage mutation={sessionMutation} success="Sesión guardada." />
        <div className="session-history">
          {visibleSessions.length ? (
            visibleSessions.map((session) => (
              <SessionRow
                key={session.id}
                session={session}
                busy={deleteSessionMutation.isPending}
                onEdit={() => setSessionDraft(sessionToDraft(session))}
                onDelete={() => deleteSessionMutation.mutate(session.id)}
              />
            ))
          ) : (
            <p className="muted-copy">Todavía no hay sesiones registradas para este juego.</p>
          )}
          {visibleSessions.length < sessionsTotal && (
            <Button
              className="session-history__more"
              size="sm"
              variant="outline"
              disabled={loadSessionsMutation.isPending}
              onClick={() => loadSessionsMutation.mutate()}
            >
              {loadSessionsMutation.isPending ? "Cargando…" : "Cargar sesiones anteriores"}
            </Button>
          )}
          <MutationMessage
            mutation={loadSessionsMutation}
            success="Historial de sesiones actualizado."
          />
        </div>
      </section>
    </div>
  );
}

function TagRow({
  tag,
  checked,
  busy,
  onToggle,
  onEdit,
  onDelete,
}: {
  tag: TagDefinition;
  checked: boolean;
  busy: boolean;
  onToggle: (checked: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="journal-tag-row">
      <Checkbox
        aria-label={`Asignar etiqueta ${tag.name}`}
        checked={checked}
        disabled={busy}
        onCheckedChange={(value) => onToggle(value === true)}
      />
      <i style={{ backgroundColor: tag.color }} />
      <span>{tag.name}</span>
      <Button size="icon-xs" variant="ghost" aria-label={`Editar ${tag.name}`} onClick={onEdit}>
        <IconEdit />
      </Button>
      <ConfirmDelete
        label={`Eliminar ${tag.name}`}
        title="¿Eliminar esta etiqueta?"
        description="Se retirará de todos los juegos, pero no se eliminará ningún juego ni nota."
        busy={busy}
        onConfirm={onDelete}
      />
    </div>
  );
}

function SessionRow({
  session,
  busy,
  onEdit,
  onDelete,
}: {
  session: GameSession;
  busy: boolean;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const progress = [session.progressBefore, session.progressAfter]
    .map((value) => (typeof value === "number" ? `${value}%` : undefined))
    .filter(Boolean)
    .join(" → ");
  return (
    <article className="session-row">
      <div>
        <strong>{formatDate(session.startedAt)}</strong>
        <span>{session.endedAt ? `Final · ${formatDate(session.endedAt)}` : "Sesión abierta"}</span>
      </div>
      <p>{session.note || "Sin nota"}</p>
      {progress && <span className="session-row__progress">Progreso {progress}</span>}
      <div>
        <Button size="icon-xs" variant="ghost" aria-label="Editar sesión" onClick={onEdit}>
          <IconEdit />
        </Button>
        <ConfirmDelete
          label="Eliminar sesión"
          title="¿Eliminar esta sesión?"
          description="La entrada desaparecerá del historial personal de forma permanente."
          busy={busy}
          onConfirm={onDelete}
        />
      </div>
    </article>
  );
}

function ConfirmDelete({
  label,
  title,
  description,
  busy,
  onConfirm,
}: {
  label: string;
  title: string;
  description: string;
  busy: boolean;
  onConfirm: () => void;
}) {
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="icon-xs" variant="ghost" aria-label={label} disabled={busy}>
          <IconTrash />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancelar</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={onConfirm}>
            Eliminar
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

function MutationMessage({
  mutation,
  success,
}: {
  mutation: { isError: boolean; isSuccess: boolean; error: unknown };
  success: string;
}) {
  if (!mutation.isError && !mutation.isSuccess) return null;
  return (
    <p className="journal-feedback" role={mutation.isError ? "alert" : "status"}>
      {mutation.isError ? getErrorMessage(mutation.error) : success}
    </p>
  );
}

function sessionToDraft(session: GameSession): SessionDraft {
  return {
    id: session.id,
    startedAt: toLocalDateTimeInput(session.startedAt),
    endedAt: session.endedAt ? toLocalDateTimeInput(session.endedAt) : "",
    progressBefore:
      typeof session.progressBefore === "number" ? String(session.progressBefore) : "",
    progressAfter: typeof session.progressAfter === "number" ? String(session.progressAfter) : "",
    note: session.note,
  };
}

function toLocalDateTimeInput(value: string): string {
  const date = new Date(value);
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}
