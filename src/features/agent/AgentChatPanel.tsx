/**
 * Hablar con Vindexa.
 *
 * # Qué es esto
 *
 * El agente de casa. Se le dice «he estado dos horas con X y voy por el 40 %» o
 * «este ya me lo pasé, bájale la prioridad» y lo hace. Corre contra un modelo
 * que ya esté sirviendo en este ordenador, y si hay varios se elige en la
 * cabecera. Si no encuentra ninguno lo dice y explica qué falta, en vez de
 * fallar sin más.
 *
 * # Por qué se enseña lo que hizo
 *
 * Cada llamada a una herramienta aparece debajo de la respuesta, con lo que
 * pidió. Un agente que ordena tu biblioteca y sólo contesta «hecho» te obliga a
 * ir a comprobarlo; enseñando los pasos, la comprobación ya está hecha. Y todo
 * lo que hace queda además en la auditoría de Ajustes → Agentes, con su botón
 * de deshacer.
 */

import { IconAlertTriangle, IconLoader2, IconRobot, IconSend, IconX } from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import type { AgentModelConfig, ChatStep, LocalModelSurvey } from "@/lib/agent-types";
import { formatRelativeDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import "./agent-chat.css";

interface Turn {
  role: "user" | "assistant";
  content: string;
  steps?: ChatStep[];
}

/**
 * Con qué modelo se habla.
 *
 * Manda lo que se haya elegido; si no hay elección, el primer servidor local
 * que declare un modelo. Quien tiene uno solo no debería decidir nada para
 * empezar a hablar, y quien tiene tres necesita poder decir cuál.
 */
function pickTarget(
  survey?: LocalModelSurvey,
  saved?: AgentModelConfig,
): { baseUrl: string; model: string } | undefined {
  // Lo guardado manda. Es lo que hace útil tener tres modelos servidos a la
  // vez: sin esto, el agente se queda con el primero que encuentre para
  // siempre.
  if (saved?.baseUrl && saved.model) {
    return { baseUrl: saved.baseUrl, model: saved.model };
  }
  const endpoint = survey?.endpoints.find((candidate) => candidate.models.length > 0);
  if (!endpoint) return undefined;
  return { baseUrl: endpoint.baseUrl, model: endpoint.models[0] as string };
}

/** Todos los modelos que se pueden elegir ahora mismo, sin repetir. */
function availableModels(survey?: LocalModelSurvey): { baseUrl: string; model: string }[] {
  const options: { baseUrl: string; model: string }[] = [];
  for (const endpoint of survey?.endpoints ?? []) {
    for (const model of endpoint.models) {
      options.push({ baseUrl: endpoint.baseUrl, model });
    }
  }
  return options;
}

export function AgentChatPanel({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<"chat" | "tasks">("chat");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string>();
  const queryClient = useQueryClient();
  const bottom = useRef<HTMLDivElement>(null);

  const survey = useQuery({
    queryKey: ["agent", "local-models"],
    queryFn: api.localModelSurvey,
    staleTime: 30_000,
  });
  const config = useQuery({ queryKey: ["agent", "model-config"], queryFn: api.vindagentConfig });
  const target = pickTarget(survey.data, config.data);
  const options = availableModels(survey.data);
  const chooseModel = useMutation({
    mutationFn: (choice: { baseUrl: string; model: string }) =>
      api.saveVindagentConfig({ ...choice, remoteAllowed: false }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["agent", "model-config"] }),
  });

  const send = useMutation({
    mutationFn: async (text: string) => {
      if (!target) throw new Error("No hay ningún modelo sirviendo en este ordenador.");
      const history = [...turns, { role: "user" as const, content: text }].map((turn) => ({
        role: turn.role,
        content: turn.content,
      }));
      return api.vindagentChat(target.baseUrl, target.model, history);
    },
    onSuccess: (turn) => {
      setTurns((current) => [
        ...current,
        { role: "assistant", content: turn.reply, steps: turn.steps },
      ]);
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, []);

  const submit = () => {
    const text = draft.trim();
    if (!text || send.isPending) return;
    setError(undefined);
    setTurns((current) => [...current, { role: "user", content: text }]);
    setDraft("");
    send.mutate(text);
  };

  return (
    <aside className="agent-chat" aria-label="Agente de Vindexa">
      <header>
        <IconRobot aria-hidden="true" />
        <div>
          <strong>Agente de Vindexa</strong>
          {/* Con un solo modelo no hay nada que elegir y un desplegable sería
              ruido; con varios, elegir es justo lo que falta. */}
          {options.length > 1 ? (
            <select
              className="agent-chat__model"
              aria-label="Modelo con el que hablar"
              value={target ? `${target.baseUrl}|${target.model}` : ""}
              disabled={chooseModel.isPending}
              onChange={(event) => {
                const [baseUrl, model] = event.currentTarget.value.split("|");
                if (baseUrl && model) chooseModel.mutate({ baseUrl, model });
              }}
            >
              {options.map((option) => (
                <option
                  key={`${option.baseUrl}|${option.model}`}
                  value={`${option.baseUrl}|${option.model}`}
                >
                  {option.model}
                </option>
              ))}
            </select>
          ) : (
            <span>
              {survey.isPending
                ? "Buscando un modelo en este ordenador…"
                : target
                  ? `${target.model} · ${config.data?.remoteAllowed ? "servicio externo" : "en local"}`
                  : "Sin modelo disponible"}
            </span>
          )}
        </div>
        <Button variant="ghost" size="icon-xs" aria-label="Cerrar el agente" onClick={onClose}>
          <IconX />
        </Button>
      </header>

      {/* Los encargos son el mismo agente diciendo lo mismo, pero en diferido:
          por eso viven aquí y no en una pantalla aparte. */}
      <nav className="agent-chat__tabs" aria-label="Secciones del agente">
        <button type="button" data-active={tab === "chat"} onClick={() => setTab("chat")}>
          Conversación
        </button>
        <button type="button" data-active={tab === "tasks"} onClick={() => setTab("tasks")}>
          Encargos
        </button>
      </nav>

      {tab === "tasks" && <ScheduledTasks hasModel={Boolean(target)} />}

      {tab === "chat" && (
        <div className="agent-chat__log">
          {!survey.isPending && !target && (
            <div className="agent-chat__empty">
              <p>
                No hay ningún modelo sirviendo en este ordenador. El agente habla sólo con lo que
                tengas en local: no manda tu biblioteca a ninguna nube.
              </p>
              <p>
                En <strong>Ajustes → Agentes</strong> puedes ver qué motores y qué modelos tienes.
                Con llama.cpp basta: arranca su servidor con un modelo y esta ventana lo encontrará
                sola.
              </p>
            </div>
          )}

          {turns.length === 0 && target && (
            <div className="agent-chat__empty">
              <p>Dile lo que has hecho y él ordena la biblioteca. Por ejemplo:</p>
              <ul>
                <li>«He estado dos horas con Hollow Knight y voy por el 40 %.»</li>
                <li>«Ese ya me lo pasé pero seguiré jugando: bájale la prioridad.»</li>
                <li>«Crea una colección "Para el finde" con los tres que tengo en Pausado.»</li>
              </ul>
            </div>
          )}

          {turns.map((turn, index) => (
            <article
              // El índice vale como clave: los turnos sólo se añaden al final y
              // nunca se reordenan ni se borran.
              key={`${turn.role}-${index}`}
              className="agent-chat__turn"
              data-role={turn.role}
            >
              <p>{turn.content}</p>
              {turn.steps && turn.steps.length > 0 && (
                <ul className="agent-chat__steps">
                  {turn.steps.map((step, position) => (
                    <li key={`${step.tool}-${position}`} data-failed={step.failed}>
                      <code>{step.tool}</code>
                      <span>{describeArguments(step.arguments)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </article>
          ))}

          {send.isPending && (
            <p className="agent-chat__thinking">
              <IconLoader2 className="is-spinning" aria-hidden="true" /> Pensando…
            </p>
          )}
          {error && (
            <p className="agent-chat__error" role="alert">
              <IconAlertTriangle aria-hidden="true" /> {error}
            </p>
          )}
          <div ref={bottom} />
        </div>
      )}

      {tab === "chat" && (
        <form
          className="agent-chat__composer"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <textarea
            aria-label="Mensaje para el agente"
            placeholder={target ? "Cuéntale qué has jugado…" : "Sin modelo disponible"}
            value={draft}
            rows={2}
            disabled={!target || send.isPending}
            onChange={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              // Intro envía; Mayús+Intro hace párrafo. Es lo que espera cualquiera
              // que haya escrito en un chat alguna vez.
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                submit();
              }
            }}
          />
          <Button type="submit" size="sm" disabled={!target || send.isPending || !draft.trim()}>
            <IconSend aria-hidden="true" /> Enviar
          </Button>
        </form>
      )}
    </aside>
  );
}

/** Los argumentos de una llamada, en una línea legible. */
function describeArguments(args: unknown): string {
  if (!args || typeof args !== "object") return "";
  return Object.entries(args as Record<string, unknown>)
    .map(([key, value]) => {
      if (value && typeof value === "object") {
        const inner = Object.values(value as Record<string, unknown>)[0];
        return `${key}: ${String(inner ?? "")}`;
      }
      return `${key}: ${String(value)}`;
    })
    .join(" · ");
}

/**
 * Encargos que el agente repite solo.
 *
 * # Por qué están aquí
 *
 * Un encargo es la misma frase que le dirías tú, pero en diferido: «cada
 * domingo, sube a Backlog lo que lleve seis meses sin tocar». Ponerlos en otra
 * pantalla los haría parecer otra cosa, y no lo son.
 *
 * # Qué se enseña de cada uno
 *
 * Lo último que hizo. Un encargo que corre solo y no cuenta nada obliga a
 * fiarse; enseñando el resultado —también cuando falla— se ve si sirve o si hay
 * que reescribirlo.
 */
function ScheduledTasks({ hasModel }: { hasModel: boolean }) {
  const queryClient = useQueryClient();
  const [instruction, setInstruction] = useState("");
  const [cadence, setCadence] = useState("semanal");
  const tasks = useQuery({ queryKey: ["agent", "tasks"], queryFn: api.listAgentTasks });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["agent", "tasks"] });

  const save = useMutation({
    mutationFn: (input: { instruction: string; cadence: string }) => api.saveAgentTask(input),
    onSuccess: () => {
      setInstruction("");
      void refresh();
    },
  });
  const toggle = useMutation({
    mutationFn: (task: { id: string; instruction: string; cadence: string; enabled: boolean }) =>
      api.saveAgentTask({
        id: task.id,
        instruction: task.instruction,
        cadence: task.cadence,
        enabled: !task.enabled,
      }),
    onSuccess: () => void refresh(),
  });
  const remove = useMutation({
    mutationFn: (taskId: string) => api.deleteAgentTask(taskId),
    onSuccess: () => void refresh(),
  });

  return (
    <div className="agent-chat__tasks">
      {!hasModel && (
        <p className="agent-chat__empty">
          Sin un modelo sirviendo, los encargos se guardan pero no corren. No se inventa un
          resultado: esperan a que haya con qué hacerlos.
        </p>
      )}

      <form
        className="agent-chat__task-form"
        onSubmit={(event) => {
          event.preventDefault();
          const text = instruction.trim();
          if (text) save.mutate({ instruction: text, cadence });
        }}
      >
        <input
          aria-label="Qué quieres que haga"
          placeholder="Cada domingo, sube a Backlog lo que lleve medio año sin tocar"
          value={instruction}
          maxLength={500}
          onChange={(event) => setInstruction(event.currentTarget.value)}
        />
        <select
          aria-label="Cada cuánto"
          value={cadence}
          onChange={(event) => setCadence(event.currentTarget.value)}
        >
          <option value="diaria">Cada día</option>
          <option value="semanal">Cada semana</option>
          <option value="mensual">Cada mes</option>
        </select>
        <Button type="submit" size="sm" disabled={!instruction.trim() || save.isPending}>
          Añadir
        </Button>
      </form>
      {save.isError && (
        <p className="agent-chat__error" role="alert">
          {getErrorMessage(save.error)}
        </p>
      )}

      {tasks.data?.length === 0 && (
        <p className="agent-chat__empty">
          Todavía no hay ninguno. Un encargo es lo mismo que le dirías por aquí, pero repetido solo.
        </p>
      )}

      <ul className="agent-chat__task-list">
        {tasks.data?.map((task) => (
          <li key={task.id} data-enabled={task.enabled}>
            <p>{task.instruction}</p>
            <span>
              {task.cadence}
              {task.lastRunAt
                ? ` · última vez ${formatRelativeDate(task.lastRunAt)}`
                : " · aún no ha corrido"}
            </span>
            {task.lastResult && <span className="agent-chat__task-result">{task.lastResult}</span>}
            <div>
              <Button
                variant="ghost"
                size="xs"
                onClick={() =>
                  toggle.mutate({
                    id: task.id,
                    instruction: task.instruction,
                    cadence: task.cadence,
                    enabled: task.enabled,
                  })
                }
              >
                {task.enabled ? "Pausar" : "Reanudar"}
              </Button>
              <Button variant="ghost" size="xs" onClick={() => remove.mutate(task.id)}>
                Borrar
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
