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
import { formatBytes, formatRelativeDate } from "@/lib/format";
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
          estados, crear colecciones o planificar. Vindexa busca sola los agentes que tengas
          instalados y se conecta a ellos; desde ahí puedes pedirle cosas hablando, también desde
          los canales que ese agente ya tenga. No se abre ningún puerto: el agente arranca un
          proceso de Vindexa y le habla por su propia tubería.
        </p>
      </div>

      <AutolinkSection />

      <div className="settings-heading">
        <h4>Modelos en este ordenador</h4>
        <p>
          Con qué cuentas para que el agente corra aquí, sin depender de ninguna nube. Es sólo un
          inventario: no se descarga nada ni se arranca nada.
        </p>
      </div>
      <LocalModelsSection />

      <div className="settings-heading">
        <h4>Conectar otro agente a mano</h4>
        <p>
          Para un agente que Vindexa todavía no reconozca. Emite un testigo, marca qué le dejas
          hacer y pégalo en su configuración de servidores MCP.
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

/**
 * Lo que Vindexa ha conectado sola.
 *
 * # Por qué no hay un botón de «conectar»
 *
 * Dar de alta un servidor MCP es copiar un comando largo con un secreto dentro,
 * y repetirlo cada vez que la aplicación se mueve o se actualiza. Eso no es una
 * decisión: es un trámite. Vindexa lo hace al arrancar y lo rehace si algo deja
 * de cuadrar. Aquí sólo se cuenta qué encontró, qué dejó conectado y desde
 * cuándo, con un interruptor por si alguien prefiere que no toque nada.
 */
function AutolinkSection() {
  const queryClient = useQueryClient();
  const estado = useQuery({
    queryKey: ["agent", "autolink"],
    queryFn: api.agentAutolinkState,
    staleTime: 10_000,
  });
  const cambiar = useMutation({
    mutationFn: (disabled: boolean) => api.setAgentAutolinkDisabled(disabled),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["agent", "autolink"] }),
  });

  if (estado.isPending) {
    return <p className="settings-hint">Buscando agentes en este ordenador…</p>;
  }
  if (estado.isError || !estado.data) {
    return <p className="settings-hint">No se pudo comprobar qué agentes hay instalados.</p>;
  }

  const { hosts, links, disabled } = estado.data;
  const instalados = hosts.filter((host) => host.path);

  return (
    <div className="agent-autolink">
      <div className="agent-autolink__switch">
        <div>
          <strong>Conectar agentes automáticamente</strong>
          <p>
            Al abrir Vindexa se buscan los agentes instalados y se conectan solos. Si la aplicación
            cambia de sitio o el testigo caduca, el enlace se rehace sin que tengas que hacer nada.
          </p>
        </div>
        <Switch
          aria-label="Conectar agentes automáticamente"
          checked={!disabled}
          disabled={cambiar.isPending}
          onCheckedChange={(value) => cambiar.mutate(!value)}
        />
      </div>

      {instalados.length === 0 ? (
        <p className="settings-hint">
          No se ha encontrado ningún agente compatible. Vindexa reconoce Hermes y Claude Code; con
          cualquier otro que hable MCP, usa el comando de más abajo.
        </p>
      ) : (
        <ul className="agent-autolink__list">
          {instalados.map((host) => {
            const link = links.find((record) => record.hostId === host.id);
            return (
              <li key={host.id} data-linked={Boolean(link)}>
                <div>
                  <strong>{host.label}</strong>
                  <span>{host.path}</span>
                </div>
                {link ? (
                  <span className="agent-autolink__state" data-kind="linked">
                    <IconCheck aria-hidden="true" /> Conectado {formatRelativeDate(link.linkedAt)}
                  </span>
                ) : (
                  <span className="agent-autolink__state" data-kind="pending">
                    {disabled ? "Automatismo apagado" : "Se conectará al reiniciar Vindexa"}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/**
 * Qué hay en este ordenador para conducir Vindexa hablando.
 *
 * # Por qué se enseña esto
 *
 * Conectar un agente no es una sola cosa. Quien ya tiene uno instalado no tiene
 * que hacer nada —Vindexa se conecta sola—; quien no lo tiene necesita saber
 * con qué cuenta: si hay un motor capaz de ejecutar un modelo, qué modelos ya
 * están descargados y cuánto le cabe a la máquina. Sin ese inventario, la única
 * respuesta posible sería «instálate algo y vuelve».
 *
 * Las cifras salen del sistema, no de una estimación: si la memoria total no se
 * puede leer, no se dice ninguna.
 */
function LocalModelsSection() {
  const survey = useQuery({
    queryKey: ["agent", "local-models"],
    queryFn: api.localModelSurvey,
    staleTime: 60_000,
  });

  if (survey.isPending) return <p className="settings-hint">Mirando qué hay instalado…</p>;
  if (survey.isError || !survey.data) {
    return <p className="settings-hint">No se pudo comprobar qué modelos hay en el disco.</p>;
  }

  const { runtimes, models, hardware } = survey.data;
  const instalados = runtimes.filter((runtime) => runtime.path);
  const cabe = hardware.usableModelBytes;

  return (
    <div className="agent-local">
      <div className="agent-local__hardware">
        <strong>Esta máquina</strong>
        <span>
          {hardware.architecture}
          {hardware.cpuCores === null ? "" : ` · ${hardware.cpuCores} núcleos`}
          {hardware.totalMemoryBytes === null
            ? " · memoria desconocida"
            : ` · ${formatBytes(hardware.totalMemoryBytes)} de memoria`}
        </span>
        <p>
          {cabe === null
            ? "No se ha podido leer la memoria del sistema, así que no se recomienda ningún tamaño: una cifra inventada aquí sólo sirve para elegir mal."
            : `Un modelo de hasta ${formatBytes(cabe)} debería ir cómodo. El resto de la memoria lo necesitan el sistema y la propia Vindexa.`}
        </p>
      </div>

      <div className="agent-local__block">
        <strong>Motores</strong>
        {instalados.length === 0 ? (
          <p className="settings-hint">
            No hay ninguno instalado. Con llama.cpp basta para ejecutar un modelo en formato GGUF.
          </p>
        ) : (
          <ul>
            {instalados.map((runtime) => (
              <li key={runtime.id}>
                <IconCheck aria-hidden="true" /> {runtime.label}{" "}
                <span>{runtime.formats.join(" · ")}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {instalados.length === 0 && <InstallRuntimeBlock />}

      <div className="agent-local__block">
        <strong>Modelos descargados</strong>
        {models.length === 0 ? (
          <p className="settings-hint">
            No se ha encontrado ninguno en las carpetas habituales de Hugging Face, LM Studio,
            Ollama ni en tus carpetas de modelos.
          </p>
        ) : (
          <ul>
            {models.slice(0, 8).map((model) => (
              <li key={model.path}>
                <span className="agent-local__name">{model.name}</span>
                <span>
                  {model.format.toUpperCase()} · {formatBytes(model.sizeBytes)}
                  {cabe !== null && model.sizeBytes > cabe ? " · no le cabe a esta máquina" : ""}
                </span>
              </li>
            ))}
            {models.length > 8 && (
              <li className="settings-hint">y {models.length - 8} más en el disco.</li>
            )}
          </ul>
        )}
      </div>

      {models.length === 0 && <SuggestionsBlock usableBytes={cabe} />}
    </div>
  );
}

/**
 * Cómo tener llama.cpp cuando no está.
 *
 * La orden se enseña antes de ejecutarla y sólo corre si alguien la pide.
 * Conectar Vindexa a un agente ya instalado toca la configuración de esa
 * aplicación y se deshace borrando una línea; instalar un paquete toca el
 * sistema entero, tarda y puede pedir contraseña. Esa diferencia es la que
 * separa lo que se hace solo de lo que se pregunta.
 */
function InstallRuntimeBlock() {
  const queryClient = useQueryClient();
  const plan = useQuery({
    queryKey: ["agent", "install-plan"],
    queryFn: api.localModelInstallPlan,
    staleTime: 60_000,
  });
  const install = useMutation({
    mutationFn: () => api.installLocalRuntime(),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent", "local-models"] });
      void queryClient.invalidateQueries({ queryKey: ["agent", "install-plan"] });
    },
  });

  if (plan.isPending || !plan.data || plan.data.alreadyInstalled) return null;

  return (
    <div className="agent-local__block">
      <strong>Instalar llama.cpp</strong>
      {plan.data.command ? (
        <>
          <p className="settings-hint">
            Se instalaría con {plan.data.manager}, ejecutando exactamente esto:
          </p>
          <code className="agent-local__command">{plan.data.command}</code>
          <div className="button-row">
            <Button size="sm" disabled={install.isPending} onClick={() => install.mutate()}>
              {install.isPending ? <IconLoader2 className="is-spinning" /> : <IconPlus />} Instalar
            </Button>
          </div>
          {install.isError && (
            <p className="settings-hint" role="alert">
              {getErrorMessage(install.error)}
            </p>
          )}
          {install.isSuccess && <p className="settings-hint">{install.data}</p>}
        </>
      ) : (
        <p className="settings-hint">
          No se ha encontrado ningún gestor de paquetes con el que instalarlo. En la página de
          llama.cpp está cómo hacerlo a mano para este sistema.
        </p>
      )}
    </div>
  );
}

/**
 * Qué descargarse cuando no hay ningún modelo.
 *
 * Los nombres salen de Hugging Face en el momento, no de una lista escrita
 * aquí: una lista fija envejece en semanas y acaba recomendando repositorios
 * que ya no existen, que es peor que no recomendar nada.
 */
function SuggestionsBlock({ usableBytes }: { usableBytes: number | null }) {
  const catalog = useQuery({
    queryKey: ["agent", "model-suggestions", usableBytes],
    queryFn: () => api.suggestLocalModels(usableBytes),
    staleTime: 10 * 60_000,
    enabled: usableBytes !== null,
  });

  if (usableBytes === null) return null;
  if (catalog.isPending) return <p className="settings-hint">Buscando modelos que te encajen…</p>;
  if (catalog.isError || !catalog.data?.suggestions.length) {
    return (
      <p className="settings-hint">
        No se pudo consultar Hugging Face ahora mismo. Cualquier modelo en formato GGUF que te quepa
        en memoria sirve.
      </p>
    );
  }

  return (
    <div className="agent-local__block">
      <strong>Qué te iría bien</strong>
      <p className="settings-hint">{catalog.data.rationale}</p>
      <ul>
        {catalog.data.suggestions.map((suggestion) => (
          <li key={suggestion.repo}>
            <span className="agent-local__name">{suggestion.repo}</span>
            <span>{suggestion.downloads.toLocaleString("es-ES")} descargas</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
