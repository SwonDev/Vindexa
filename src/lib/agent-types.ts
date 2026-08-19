/**
 * Contrato del puente para agentes externos.
 *
 * Refleja exactamente lo que serializa `src-tauri/src/agent/`. La especificación
 * completa —catálogo de intenciones, ámbitos, confirmación, deshacer y
 * auditoría— vive en `docs/AGENT_BRIDGE.md`.
 */

export type AgentScope =
  | "biblioteca:leer"
  | "biblioteca:escribir"
  | "sesiones:escribir"
  | "colecciones:escribir"
  | "listas:escribir"
  | "planificador:escribir"
  | "avisos:escribir";

/** Un juego se identifica por AppID o por nombre en lenguaje natural. */
export interface GameSelector {
  appId?: number;
  name?: string;
}

export interface EntitySelector {
  id?: string;
  name?: string;
}

export type AgentQuery =
  | { kind: "biblioteca"; text?: string; statusId?: string | null; limit?: number | null }
  | { kind: "juego"; game: GameSelector }
  | { kind: "estados" }
  | { kind: "colecciones" }
  | { kind: "listas" }
  | { kind: "planificador" }
  | { kind: "avisos" }
  | { kind: "sesiones"; game: GameSelector; limit?: number | null }
  | { kind: "auditoria"; limit?: number | null };

export type AgentIntent =
  | {
      intent: "registrar_sesion";
      game: GameSelector;
      minutes: number;
      startedAt?: string | null;
      progress?: number | null;
      note?: string;
    }
  | {
      intent: "marcar_terminado";
      game: GameSelector;
      completedOn?: string | null;
      keepPlayable?: boolean;
      priority?: number | null;
    }
  | { intent: "cambiar_estado"; game: GameSelector; statusId: string }
  | {
      intent: "ajustar_prioridad";
      game: GameSelector;
      priority?: number | null;
      delta?: number | null;
    }
  | { intent: "fijar"; game: GameSelector; pinned: boolean }
  | { intent: "seguir"; game: GameSelector; tracking: boolean }
  | { intent: "valorar"; game: GameSelector; rating?: number | null }
  | { intent: "anotar"; game: GameSelector; note: string; append?: boolean }
  | { intent: "fijar_proxima_accion"; game: GameSelector; action?: string | null }
  | { intent: "fijar_checkpoint"; game: GameSelector; checkpoint?: string | null }
  | {
      intent: "crear_coleccion";
      name: string;
      description?: string;
      color?: string;
      icon?: string;
      games?: GameSelector[];
    }
  | { intent: "añadir_a_coleccion"; collection: EntitySelector; games: GameSelector[] }
  | { intent: "quitar_de_coleccion"; collection: EntitySelector; games: GameSelector[] }
  | {
      intent: "crear_lista_curada";
      name: string;
      description?: string;
      kind?: "manual" | "wishlist" | "backlog" | "showcase";
      accent?: "cyan" | "blue" | "teal" | "lime" | "amber" | "rose" | "violet" | "slate";
      icon?: string;
      pinned?: boolean;
    }
  | {
      intent: "añadir_a_lista";
      list: EntitySelector;
      games: GameSelector[];
      note?: string;
      highlight?: boolean;
    }
  | {
      intent: "planificar";
      game: GameSelector;
      columnId: string;
      targetDate?: string | null;
      estimatedMinutes?: number | null;
    }
  | { intent: "programar_aviso"; game: GameSelector; dueAt: string; note?: string }
  | { intent: "consultar"; query: AgentQuery };

export interface AffectedGame {
  appId: number;
  title: string;
}

export interface GameCandidate {
  appId: number;
  title: string;
  score: number;
}

export type AgentOutcome =
  | {
      outcome: "applied";
      auditId: string;
      undoToken: string | null;
      affected: AffectedGame[];
      summary: string;
    }
  | {
      outcome: "pending_confirmation";
      auditId: string;
      reason: string;
      affected: AffectedGame[];
      summary: string;
    }
  | { outcome: "needs_game_choice"; auditId: string; query: string; candidates: GameCandidate[] }
  | { outcome: "answer"; auditId: string; data: unknown }
  | { outcome: "rejected"; auditId: string }
  | { outcome: "undone"; auditId: string; restored: number };

export type AgentAuditResult = "pending" | "applied" | "rejected" | "failed" | "undone";

export interface AgentAuditEntry {
  id: string;
  clientId: string | null;
  clientName: string | null;
  intent: string;
  utterance: string;
  arguments: unknown;
  result: AgentAuditResult;
  affected: AffectedGame[];
  undoable: boolean;
  errorMessage: string | null;
  createdAt: string;
  completedAt: string | null;
}

export interface AgentClientSummary {
  id: string;
  name: string;
  kind: "hermes" | "generic";
  scopes: AgentScope[];
  enabled: boolean;
  lastSeenAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface IssuedAgentClient {
  client: AgentClientSummary;
  /** Se entrega una sola vez: Vindexa guarda únicamente su hash con sal. */
  token: string;
}

export interface NewAgentClient {
  name: string;
  kind?: "hermes" | "generic";
  scopes: AgentScope[];
}

export interface AgentRequest {
  token: string;
  utterance?: string;
  intent: AgentIntent;
}

/**
 * Un agente compatible que puede estar instalado en este ordenador.
 *
 * `path` a `null` significa que no está: entonces no se emite ningún testigo
 * para él, porque una credencial que nadie tiene sólo sirve para caducar mal.
 */
export interface AgentHost {
  id: string;
  label: string;
  path: string | null;
  /** Comando que se ejecutaría, sin el testigo dentro. */
  commandPreview: string;
}

/** Alta que Vindexa dejó hecha en un agente. */
export interface AgentLinkRecord {
  hostId: string;
  clientId: string;
  /** Ruta registrada. Si deja de coincidir, el alta se rehace sola. */
  command: string;
  linkedAt: string;
}

/** Qué hay instalado, qué está conectado y si el automatismo sigue encendido. */
export interface AgentAutolinkStatus {
  disabled: boolean;
  links: AgentLinkRecord[];
  hosts: AgentHost[];
}

/** Motor capaz de ejecutar un modelo en local. */
export interface LocalRuntime {
  id: string;
  label: string;
  /** `null` significa que no está instalado. */
  path: string | null;
  formats: string[];
}

/** Un modelo ya descargado en el disco. */
export interface LocalModel {
  name: string;
  path: string;
  format: "gguf" | "mlx" | string;
  sizeBytes: number;
  /** Carpeta conocida donde apareció. */
  source: string;
}

/** Lo que la máquina puede permitirse. `null` es «no se sabe», nunca cero. */
export interface LocalHardware {
  totalMemoryBytes: number | null;
  cpuCores: number | null;
  architecture: string;
  usableModelBytes: number | null;
}

/** Servidor de inferencia que ya está corriendo en el bucle local. */
export interface InferenceEndpoint {
  baseUrl: string;
  label: string;
  models: string[];
}

export interface LocalModelSurvey {
  runtimes: LocalRuntime[];
  models: LocalModel[];
  hardware: LocalHardware;
  endpoints: InferenceEndpoint[];
}

/** Un mensaje de la conversación con el agente de casa. */
export interface ChatMessage {
  role: "user" | "assistant" | "tool";
  content: string;
  tool?: string;
}

/** Una llamada a herramienta, con lo que pidió y cómo salió. */
export interface ChatStep {
  tool: string;
  arguments: unknown;
  result: string;
  failed: boolean;
}

export interface ChatTurn {
  reply: string;
  steps: ChatStep[];
}

/** Un modelo que se puede descargar, propuesto por Hugging Face. */
export interface ModelSuggestion {
  repo: string;
  downloads: number;
  likes: number;
}

export interface CatalogSuggestions {
  /** Por qué se propone ese tamaño. Vacío si no se pudo decidir. */
  rationale: string;
  suggestions: ModelSuggestion[];
}

/** Qué haría falta para tener llama.cpp aquí. */
export interface InstallPlan {
  alreadyInstalled: boolean;
  /** Gestor encontrado. `null` significa que habría que instalarlo a mano. */
  manager: string | null;
  /** La orden exacta, para poder leerla antes de ejecutarla. */
  command: string | null;
}

/** Con qué modelo habla el agente de casa. */
export interface AgentModelConfig {
  /** Vacío significa «el que se detecte». */
  baseUrl: string;
  model: string;
  /** Sólo `true` si se aceptó a sabiendas usar un servicio de fuera. */
  remoteAllowed: boolean;
  /** Si hay una clave guardada en el llavero. El valor no viaja nunca. */
  hasApiKey: boolean;
}

export interface SaveAgentModelConfig {
  baseUrl: string;
  model: string;
  remoteAllowed: boolean;
  /** Clave nueva. Omitir deja la que hubiera; cadena vacía la borra. */
  apiKey?: string;
}

/** Un encargo que el agente repite solo. */
export interface ScheduledTask {
  id: string;
  instruction: string;
  cadence: "diaria" | "semanal" | "mensual" | string;
  enabled: boolean;
  lastRunAt: string | null;
  /** Resumen de lo último que hizo, o del error si falló. */
  lastResult: string | null;
  createdAt: string;
}

export interface SaveScheduledTask {
  id?: string;
  instruction: string;
  cadence: string;
  enabled?: boolean;
}
