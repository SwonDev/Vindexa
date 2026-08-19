/**
 * Agentes que pueden actuar sobre Vindexa en tu nombre.
 *
 * # Qué es un agente aquí
 *
 * Un programa —Hermes, un guion propio— al que le das un testigo y una lista de
 * permisos para que ordene tu biblioteca sin que tengas que abrirla: «he jugado
 * dos horas a esto y voy por el 40 %», «este ya lo terminé, bájale la
 * prioridad». El puente que lo atiende vive entero en Rust y lleva tiempo
 * escrito; lo que faltaba era esta pantalla, porque sin ella no había forma de
 * emitir el testigo.
 *
 * # Las cuatro barreras, y por qué siguen ahí
 *
 * 1. **No hay puerto de escucha.** Vindexa no abre ningún socket, tampoco en
 *    `127.0.0.1`: sería una puerta accesible a cualquier proceso local y a
 *    cualquier página capaz de llamar al bucle local. El agente entra por un
 *    proceso acompañante que lanza la propia aplicación.
 * 2. **El testigo se enseña una sola vez.** Vindexa guarda su huella con sal,
 *    no el testigo: si se pierde, se rota, no se recupera.
 * 3. **Permisos por ámbito.** Un agente que sólo puede registrar sesiones no
 *    puede tocar tus colecciones, aunque lo pida.
 * 4. **Todo queda registrado y casi todo se puede deshacer.** Cada orden deja
 *    su rastro con lo que tocó, y lo destructivo espera a que lo apruebes tú:
 *    un agente no puede confirmar sus propias acciones.
 */

import {
  IconAlertTriangle,
  IconArrowBackUp,
  IconCheck,
  IconCopy,
  IconLoader2,
  IconPlus,
  IconRobot,
  IconTrash,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { AgentClientSummary, AgentScope, IssuedAgentClient } from "@/lib/agent-types";
import { formatRelativeDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import "./agents-panel.css";

const CLIENTS_KEY = ["agent", "clients"] as const;
const AUDIT_KEY = ["agent", "audit"] as const;

/**
 * Los ámbitos, con lo que de verdad permiten.
 *
 * La descripción no repite el nombre técnico: dice qué puede hacer el agente si
 * marcas esa casilla, que es lo único que hay que entender para decidir.
 */
const SCOPES: { value: AgentScope; label: string; detail: string }[] = [
  {
    value: "biblioteca:leer",
    label: "Consultar la biblioteca",
    detail: "Buscar juegos y leer su estado. No cambia nada.",
  },
  {
    value: "biblioteca:escribir",
    label: "Cambiar estado y progreso",
    detail: "Clasificar, valorar, anotar, fijar prioridad y marcar terminado.",
  },
  {
    value: "sesiones:escribir",
    label: "Registrar sesiones de juego",
    detail: "Apuntar cuánto has jugado y por dónde vas.",
  },
  {
    value: "colecciones:escribir",
    label: "Gestionar colecciones",
    detail: "Crear colecciones y mover juegos entre ellas.",
  },
  {
    value: "listas:escribir",
    label: "Gestionar listas curadas",
    detail: "Crear listas y añadirles juegos.",
  },
  {
    value: "planificador:escribir",
    label: "Planificar",
    detail: "Colocar juegos en el planificador y ordenarlos.",
  },
  {
    value: "avisos:escribir",
    label: "Programar avisos",
    detail: "Crear recordatorios sobre un juego.",
  },
];

/** Ámbitos con los que arranca un agente nuevo: leer y poco más. */
const DEFAULT_SCOPES: AgentScope[] = ["biblioteca:leer"];

export function AgentsPanel() {
  const queryClient = useQueryClient();
  const nameId = useId();
  const [name, setName] = useState("");
  const [scopes, setScopes] = useState<AgentScope[]>(DEFAULT_SCOPES);
  const [issued, setIssued] = useState<IssuedAgentClient>();
  const [copied, setCopied] = useState(false);
  const [notice, setNotice] = useState<{ kind: "success" | "error"; message: string }>();

  const clients = useQuery({ queryKey: CLIENTS_KEY, queryFn: api.listAgentClients });
  const audit = useQuery({ queryKey: AUDIT_KEY, queryFn: () => api.listAgentAudit(20) });

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: CLIENTS_KEY });
    void queryClient.invalidateQueries({ queryKey: AUDIT_KEY });
    // Lo que un agente cambia se ve en la biblioteca, no sólo aquí.
    void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
    void queryClient.invalidateQueries({ queryKey: ["games"] });
  };

  const issue = useMutation({
    mutationFn: () => api.issueAgentClient({ name: name.trim(), kind: "hermes", scopes }),
    onSuccess: (result) => {
      setIssued(result);
      setCopied(false);
      setName("");
      setScopes(DEFAULT_SCOPES);
      refresh();
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const rotate = useMutation({
    mutationFn: (clientId: string) => api.rotateAgentToken(clientId),
    onSuccess: (result) => {
      setIssued(result);
      setCopied(false);
      setNotice({
        kind: "success",
        message: "Testigo rotado. El anterior ha dejado de servir ahora mismo.",
      });
      refresh();
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const setEnabled = useMutation({
    mutationFn: ({ clientId, enabled }: { clientId: string; enabled: boolean }) =>
      api.setAgentClientEnabled(clientId, enabled),
    onSuccess: refresh,
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const revoke = useMutation({
    mutationFn: (clientId: string) => api.revokeAgentClient(clientId),
    onSuccess: () => {
      setNotice({ kind: "success", message: "Agente revocado. Su testigo ya no vale." });
      refresh();
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const undo = useMutation({
    mutationFn: (undoToken: string) => api.agentUndo(undoToken),
    onSuccess: () => {
      setNotice({ kind: "success", message: "Acción deshecha." });
      refresh();
    },
    onError: (error) => setNotice({ kind: "error", message: getErrorMessage(error) }),
  });

  const busy = issue.isPending || rotate.isPending || revoke.isPending;

  return (
    <section className="settings-section agents-panel">
      <div className="settings-heading">
        <h3>Agentes</h3>
        <p>
          Un agente puede ordenar tu biblioteca en tu nombre: registrar lo que has jugado, cambiar
          estados, crear colecciones o planificar. Le das un testigo y decides exactamente qué puede
          tocar. Vindexa no abre ningún puerto para esto: el agente entra por un proceso que lanza
          la propia aplicación.
        </p>
      </div>

      {notice && (
        <div className="inline-notice" data-kind={notice.kind}>
          {notice.kind === "success" ? <IconCheck /> : <IconAlertTriangle />}
          <span>{notice.message}</span>
        </div>
      )}

      {issued && (
        <div className="agent-token" role="alert">
          <p>
            <strong>Copia el testigo de {issued.client.name} ahora.</strong> Es la única vez que se
            enseña: Vindexa guarda sólo su huella, así que si lo pierdes hay que rotarlo.
          </p>
          <div className="agent-token__value">
            <code>{issued.token}</code>
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                void navigator.clipboard.writeText(issued.token).then(() => setCopied(true));
              }}
            >
              {copied ? <IconCheck /> : <IconCopy />} {copied ? "Copiado" : "Copiar"}
            </Button>
          </div>
          <Button size="sm" variant="ghost" onClick={() => setIssued(undefined)}>
            Ya lo tengo guardado
          </Button>
        </div>
      )}

      <form
        className="agent-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (name.trim() && scopes.length) issue.mutate();
        }}
      >
        <label htmlFor={nameId}>
          <span>Nombre del agente</span>
          <Input
            id={nameId}
            value={name}
            placeholder="Hermes"
            maxLength={80}
            onChange={(event) => setName(event.target.value)}
          />
        </label>

        <fieldset className="agent-scopes">
          <legend>Qué le dejas hacer</legend>
          {SCOPES.map((scope) => (
            <label key={scope.value} className="agent-scope" htmlFor={`${nameId}-${scope.value}`}>
              <Switch
                id={`${nameId}-${scope.value}`}
                checked={scopes.includes(scope.value)}
                onCheckedChange={(checked) =>
                  setScopes((current) =>
                    checked
                      ? [...current, scope.value]
                      : current.filter((value) => value !== scope.value),
                  )
                }
              />
              <span>
                <strong>{scope.label}</strong>
                <small>{scope.detail}</small>
              </span>
            </label>
          ))}
        </fieldset>

        <Button type="submit" size="sm" disabled={busy || !name.trim() || !scopes.length}>
          {issue.isPending ? <IconLoader2 className="is-spinning" /> : <IconPlus />} Emitir testigo
        </Button>
      </form>

      <div className="agent-list">
        <h4>Agentes con acceso</h4>
        {clients.isPending ? (
          <p className="settings-hint">Consultando…</p>
        ) : clients.data?.length ? (
          clients.data.map((client) => <AgentRow key={client.id} client={client} />)
        ) : (
          <p className="settings-hint">
            Todavía no has dado acceso a ningún agente. Emite un testigo arriba cuando quieras
            conectar uno.
          </p>
        )}
      </div>

      <div className="agent-audit">
        <h4>Lo que han hecho</h4>
        <p className="settings-hint">
          Cada orden queda registrada con lo que tocó. Lo que un agente aplicó, lo puedes deshacer
          tú.
        </p>
        {audit.data?.length ? (
          <ul>
            {audit.data.map((entry) => (
              <li key={entry.id} data-result={entry.result}>
                <div>
                  <strong>{entry.intent}</strong>
                  {entry.utterance && <em>«{entry.utterance}»</em>}
                  <span>
                    {entry.clientName ?? "Agente retirado"} · {formatRelativeDate(entry.createdAt)}
                    {entry.affected.length > 0 &&
                      ` · ${entry.affected.map((game) => game.title).join(", ")}`}
                  </span>
                  {entry.errorMessage && <span data-kind="error">{entry.errorMessage}</span>}
                </div>
                {entry.undoable && (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={undo.isPending}
                    onClick={() => undo.mutate(entry.id)}
                  >
                    <IconArrowBackUp /> Deshacer
                  </Button>
                )}
              </li>
            ))}
          </ul>
        ) : (
          <p className="settings-hint">Ninguna orden todavía.</p>
        )}
      </div>
    </section>
  );

  function AgentRow({ client }: { client: AgentClientSummary }) {
    return (
      <article className="agent-row" data-enabled={client.enabled}>
        <div className="agent-row__identity">
          <IconRobot aria-hidden="true" />
          <div>
            <strong>{client.name}</strong>
            <span>
              {client.lastSeenAt
                ? `Última orden ${formatRelativeDate(client.lastSeenAt)}`
                : "Todavía no ha actuado"}
            </span>
          </div>
        </div>
        <ul className="agent-row__scopes">
          {client.scopes.map((scope) => (
            <li key={scope}>{SCOPES.find((item) => item.value === scope)?.label ?? scope}</li>
          ))}
        </ul>
        <div className="agent-row__actions">
          <label htmlFor={`agent-enabled-${client.id}`}>
            <Switch
              id={`agent-enabled-${client.id}`}
              checked={client.enabled}
              onCheckedChange={(enabled) => setEnabled.mutate({ clientId: client.id, enabled })}
            />
            <span>{client.enabled ? "Activo" : "En pausa"}</span>
          </label>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => rotate.mutate(client.id)}
          >
            Rotar testigo
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => revoke.mutate(client.id)}
          >
            <IconTrash /> Revocar
          </Button>
        </div>
      </article>
    );
  }
}
