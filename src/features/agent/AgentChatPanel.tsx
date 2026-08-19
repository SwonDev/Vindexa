/**
 * Hablar con Vindexa.
 *
 * # Qué es esto
 *
 * El agente de casa. Se le dice «he estado dos horas con X y voy por el 40 %» o
 * «este ya me lo pasé, bájale la prioridad» y lo hace. Corre contra un modelo
 * que ya esté sirviendo en este ordenador: no manda nada a ninguna nube, y si
 * no encuentra ninguno lo dice y explica qué falta, en vez de fallar sin más.
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
import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import type { ChatStep, LocalModelSurvey } from "@/lib/agent-types";
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
 * Se elige el primer servidor que declare al menos un modelo. No se pregunta
 * cuál: quien tiene varios puede elegirlo en Ajustes, y quien tiene uno no
 * debería tener que decidir nada para empezar a hablar.
 */
function pickTarget(survey?: LocalModelSurvey): { baseUrl: string; model: string } | undefined {
  const endpoint = survey?.endpoints.find((candidate) => candidate.models.length > 0);
  if (!endpoint) return undefined;
  return { baseUrl: endpoint.baseUrl, model: endpoint.models[0] as string };
}

export function AgentChatPanel({ onClose }: { onClose: () => void }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string>();
  const bottom = useRef<HTMLDivElement>(null);

  const survey = useQuery({
    queryKey: ["agent", "local-models"],
    queryFn: api.localModelSurvey,
    staleTime: 30_000,
  });
  const target = pickTarget(survey.data);

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
          <span>
            {survey.isPending
              ? "Buscando un modelo en este ordenador…"
              : target
                ? `${target.model} · en local`
                : "Sin modelo disponible"}
          </span>
        </div>
        <Button variant="ghost" size="icon-xs" aria-label="Cerrar el agente" onClick={onClose}>
          <IconX />
        </Button>
      </header>

      <div className="agent-chat__log">
        {!survey.isPending && !target && (
          <div className="agent-chat__empty">
            <p>
              No hay ningún modelo sirviendo en este ordenador. El agente habla sólo con lo que
              tengas en local: no manda tu biblioteca a ninguna nube.
            </p>
            <p>
              En <strong>Ajustes → Agentes</strong> puedes ver qué motores y qué modelos tienes. Con
              llama.cpp basta: arranca su servidor con un modelo y esta ventana lo encontrará sola.
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
