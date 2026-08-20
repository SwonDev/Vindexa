import type {
  AppSection,
  CollectionSummary,
  GameSummary,
  LibraryView,
  ShortcutAction,
  ShortcutBindings,
  StatusDefinition,
} from "@/lib/types";

/**
 * Atajos de Vindexa.
 *
 * Hay dos familias y conviene no confundirlas:
 *
 * - **Navegación** (`ShortcutAction`): las siete acciones que ya viajaban a
 *   SQLite dentro de `AppPreferences.shortcuts`. Su formato lo valida también
 *   Rust, así que aquí sólo se admiten combinaciones que el backend acepta.
 * - **Locales** (`LocalShortcutAction`): las acciones operativas nuevas —y la
 *   navegación a Deseados, que llegó después—. El esquema de Rust es una
 *   `struct` de siete campos con nombre, de modo que hoy no pueden persistirse
 *   ahí sin migrar el backend; se guardan en `localStorage`, igual que ya hace
 *   `library-session.ts` con el estado de la pantalla. El informe de esta tarea
 *   incluye el diff exacto para moverlas a SQLite cuando se toque `src-tauri`.
 */

// ── Acciones de navegación (persistidas en SQLite) ─────────────────────────

/**
 * `search` pasa a `Mod+F`: `Mod+K` es ahora la paleta de comandos, que es lo
 * que ese gesto significa en cualquier aplicación de escritorio moderna.
 */
export const DEFAULT_SHORTCUTS: ShortcutBindings = {
  library: "Mod+1",
  planner: "Mod+2",
  wishlist: "Mod+5",
  collections: "Mod+3",
  tracking: "Mod+4",
  search: "Mod+F",
  sync: "Mod+Shift+S",
  closePanel: "Escape",
};

/**
 * Valor con el que `search` salía de fábrica antes de existir la paleta. Si una
 * base de datos todavía lo trae, se migra a `Mod+F` al leerla: de lo contrario
 * `Mod+K` quedaría asignado dos veces y la paleta no abriría nunca.
 */
export const LEGACY_SEARCH_SHORTCUT = "Mod+K";

export const SHORTCUT_ACTIONS: readonly ShortcutAction[] = [
  "library",
  "planner",
  "wishlist",
  "collections",
  "tracking",
  "search",
  "sync",
  "closePanel",
];

// ── Acciones locales (persistidas en localStorage) ────────────────────────

export type LocalShortcutAction =
  /**
   * Entrar al modo sofá. Es navegación, pero se guarda en el navegador porque
   * la `struct` de preferencias de Rust no tiene ese campo. Deseados **sí** lo
   * tiene, así que vive con el resto de la navegación: tenerlo en los dos
   * sitios dejaba «Mod+5» ocupado por un enlace que nadie escuchaba.
   */
  | "couchMode"
  | "commandPalette"
  | "primaryAction"
  | "openDetail"
  | "openStore"
  | "revealInstallation"
  | "requestUninstall"
  | "togglePinned"
  | "toggleTracking"
  | "statusForward"
  | "statusBackward"
  | "statusSlot1"
  | "statusSlot2"
  | "statusSlot3"
  | "statusSlot4"
  | "statusSlot5"
  | "addToPlanner"
  | "undo"
  | "priorityUp"
  | "priorityDown"
  | "addToCollection"
  | "copyTitle"
  | "copyAppId"
  | "focusUp"
  | "focusDown"
  | "focusLeft"
  | "focusRight"
  | "focusFirst"
  | "focusLast"
  | "extendUp"
  | "extendDown"
  | "extendLeft"
  | "extendRight"
  | "selectAll";

export type AnyShortcutAction = ShortcutAction | LocalShortcutAction;

/** Mapa completo con las dos familias ya fusionadas. */
export type ShortcutMap = Record<AnyShortcutAction, string>;

export const DEFAULT_LOCAL_SHORTCUTS: Record<LocalShortcutAction, string> = {
  couchMode: "Mod+Shift+M",
  commandPalette: "Mod+K",
  primaryAction: "Enter",
  openDetail: "Space",
  openStore: "Mod+T",
  revealInstallation: "Mod+Shift+R",
  // `Supr` a secas es demasiado fácil de pulsar sin querer sobre la ficha
  // enfocada mientras se navega con las flechas.
  requestUninstall: "Mod+Delete",
  togglePinned: "Mod+D",
  toggleTracking: "Mod+E",
  statusForward: "Alt+ArrowRight",
  statusBackward: "Alt+ArrowLeft",
  // Ciclar sirve para explorar; para mover un juego a un estado concreto hace
  // falta una tecla que no dependa de cuántos pasos hay hasta él.
  statusSlot1: "Alt+1",
  statusSlot2: "Alt+2",
  statusSlot3: "Alt+3",
  statusSlot4: "Alt+4",
  statusSlot5: "Alt+5",
  addToPlanner: "Mod+Shift+P",
  undo: "Mod+Z",
  priorityUp: "Alt+ArrowUp",
  priorityDown: "Alt+ArrowDown",
  addToCollection: "Mod+Shift+A",
  copyTitle: "Mod+C",
  copyAppId: "Mod+Shift+C",
  focusUp: "ArrowUp",
  focusDown: "ArrowDown",
  focusLeft: "ArrowLeft",
  focusRight: "ArrowRight",
  focusFirst: "Home",
  focusLast: "End",
  extendUp: "Shift+ArrowUp",
  extendDown: "Shift+ArrowDown",
  extendLeft: "Shift+ArrowLeft",
  extendRight: "Shift+ArrowRight",
  selectAll: "Mod+A",
};

export const LOCAL_SHORTCUT_ACTIONS: readonly LocalShortcutAction[] = Object.keys(
  DEFAULT_LOCAL_SHORTCUTS,
) as LocalShortcutAction[];

export const DEFAULT_SHORTCUT_MAP: ShortcutMap = {
  ...DEFAULT_SHORTCUTS,
  ...DEFAULT_LOCAL_SHORTCUTS,
};

// ── Catálogo para Ajustes ──────────────────────────────────────────────────

/** Ámbito de una acción: determina cuándo puede dispararse y cómo se agrupa. */
export type ShortcutScope = "navigation" | "library" | "game";

export interface ShortcutDescriptor {
  action: AnyShortcutAction;
  scope: ShortcutScope;
  label: string;
  /** Dónde se guarda: el backend valida más estrictamente que el navegador. */
  persistence: "backend" | "local";
}

export const SHORTCUT_SCOPE_LABELS: Record<ShortcutScope, string> = {
  navigation: "Navegación",
  library: "Biblioteca",
  game: "Juego enfocado",
};

export const SHORTCUT_SCOPE_HINTS: Record<ShortcutScope, string> = {
  navigation: "Mueven la aplicación entre secciones y paneles.",
  library: "Actúan sobre la rejilla o la lista: foco, selección y paleta.",
  game: "Actúan sobre el juego que tiene el foco en la biblioteca.",
};

export const SHORTCUT_CATALOGUE: readonly ShortcutDescriptor[] = [
  { action: "library", scope: "navigation", label: "Biblioteca", persistence: "backend" },
  { action: "planner", scope: "navigation", label: "Planificador", persistence: "backend" },
  { action: "collections", scope: "navigation", label: "Colecciones", persistence: "backend" },
  { action: "tracking", scope: "navigation", label: "Seguimiento", persistence: "backend" },
  { action: "wishlist", scope: "navigation", label: "Deseados", persistence: "backend" },
  { action: "couchMode", scope: "navigation", label: "Modo sofá", persistence: "local" },
  {
    action: "search",
    scope: "navigation",
    label: "Buscar en la biblioteca",
    persistence: "backend",
  },
  { action: "sync", scope: "navigation", label: "Sincronizar con Steam", persistence: "backend" },
  {
    action: "closePanel",
    scope: "navigation",
    label: "Cerrar panel activo",
    persistence: "backend",
  },
  {
    action: "commandPalette",
    scope: "library",
    label: "Abrir la paleta de comandos",
    persistence: "local",
  },
  { action: "focusUp", scope: "library", label: "Mover el foco arriba", persistence: "local" },
  { action: "focusDown", scope: "library", label: "Mover el foco abajo", persistence: "local" },
  {
    action: "focusLeft",
    scope: "library",
    label: "Mover el foco a la izquierda",
    persistence: "local",
  },
  {
    action: "focusRight",
    scope: "library",
    label: "Mover el foco a la derecha",
    persistence: "local",
  },
  { action: "focusFirst", scope: "library", label: "Ir al primer juego", persistence: "local" },
  { action: "focusLast", scope: "library", label: "Ir al último juego", persistence: "local" },
  {
    action: "extendUp",
    scope: "library",
    label: "Extender la selección arriba",
    persistence: "local",
  },
  {
    action: "extendDown",
    scope: "library",
    label: "Extender la selección abajo",
    persistence: "local",
  },
  {
    action: "extendLeft",
    scope: "library",
    label: "Extender la selección a la izquierda",
    persistence: "local",
  },
  {
    action: "extendRight",
    scope: "library",
    label: "Extender la selección a la derecha",
    persistence: "local",
  },
  { action: "selectAll", scope: "library", label: "Seleccionar todo", persistence: "local" },
  {
    action: "primaryAction",
    scope: "game",
    label: "Acción principal (jugar o abrir ficha)",
    persistence: "local",
  },
  { action: "openDetail", scope: "game", label: "Abrir la ficha", persistence: "local" },
  { action: "openStore", scope: "game", label: "Abrir la tienda integrada", persistence: "local" },
  {
    action: "revealInstallation",
    scope: "game",
    label: "Revelar la instalación",
    persistence: "local",
  },
  {
    action: "requestUninstall",
    scope: "game",
    label: "Solicitar la desinstalación",
    persistence: "local",
  },
  { action: "togglePinned", scope: "game", label: "Fijar o desfijar", persistence: "local" },
  { action: "toggleTracking", scope: "game", label: "Marcar seguimiento", persistence: "local" },
  { action: "statusForward", scope: "game", label: "Estado siguiente", persistence: "local" },
  { action: "statusBackward", scope: "game", label: "Estado anterior", persistence: "local" },
  { action: "statusSlot1", scope: "game", label: "Mover al 1.er estado", persistence: "local" },
  { action: "statusSlot2", scope: "game", label: "Mover al 2.º estado", persistence: "local" },
  { action: "statusSlot3", scope: "game", label: "Mover al 3.er estado", persistence: "local" },
  { action: "statusSlot4", scope: "game", label: "Mover al 4.º estado", persistence: "local" },
  { action: "statusSlot5", scope: "game", label: "Mover al 5.º estado", persistence: "local" },
  { action: "addToPlanner", scope: "game", label: "Añadir al plan", persistence: "local" },
  { action: "undo", scope: "library", label: "Deshacer el último cambio", persistence: "local" },
  { action: "priorityUp", scope: "game", label: "Subir la prioridad", persistence: "local" },
  { action: "priorityDown", scope: "game", label: "Bajar la prioridad", persistence: "local" },
  {
    action: "addToCollection",
    scope: "game",
    label: "Añadir a una colección",
    persistence: "local",
  },
  { action: "copyTitle", scope: "game", label: "Copiar el título", persistence: "local" },
  { action: "copyAppId", scope: "game", label: "Copiar el AppID", persistence: "local" },
];

/**
 * Acción de teclado que lleva a cada sección. Es un `Record` completo a
 * propósito: si mañana aparece otra sección, el compilador obliga a decidir si
 * merece atajo en vez de dejarla en silencio sin él.
 */
const SECTION_SHORTCUT_ACTIONS: Record<AppSection, AnyShortcutAction | undefined> = {
  library: "library",
  planner: "planner",
  wishlist: "wishlist",
  collections: "collections",
  tracking: "tracking",
  couch: "couchMode",
};

/** Combinación asignada a una sección, o `undefined` si no tiene ninguna. */
export function sectionShortcut(bindings: ShortcutMap, section: AppSection): string | undefined {
  const action = SECTION_SHORTCUT_ACTIONS[section];
  return action ? bindings[action] || undefined : undefined;
}

export function shortcutScope(action: AnyShortcutAction): ShortcutScope {
  return SHORTCUT_CATALOGUE.find((entry) => entry.action === action)?.scope ?? "navigation";
}

export function shortcutName(action: AnyShortcutAction): string {
  return SHORTCUT_CATALOGUE.find((entry) => entry.action === action)?.label ?? action;
}

// ── Normalización de eventos ───────────────────────────────────────────────

const RESERVED_SHORTCUTS: Readonly<Record<string, string>> = {
  "Mod+Comma": "Abrir Ajustes",
};

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift"]);

/**
 * Teclas que valen por sí solas. Sin ellas no hay `Intro`, `Espacio` ni flechas
 * y la biblioteca no se puede recorrer sin ratón, que es justo el problema que
 * esta tanda arregla.
 */
const STANDALONE_KEYS = new Set([
  "Escape",
  "Enter",
  "Space",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);

const NAMED_KEYS = new Set([
  "Escape",
  "Enter",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);

/** Teclas con nombre que Rust admite hoy en `is_valid_shortcut`. */
const BACKEND_NAMED_KEYS = new Set([
  "Escape",
  "Enter",
  "Backspace",
  "Delete",
  "Space",
  "Comma",
  "Slash",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);

function isFunctionKey(key: string): boolean {
  return /^F(?:[1-9]|1[0-2])$/.test(key);
}

function normalizeKey(key: string): string | undefined {
  if (MODIFIER_KEYS.has(key) || key === "Dead" || key === "Unidentified") return undefined;
  if (key.length === 1 && /[a-z0-9]/i.test(key)) return key.toUpperCase();
  if (key === " ") return "Space";
  if (key === ",") return "Comma";
  if (key === "/") return "Slash";
  if (isFunctionKey(key)) return key;
  if (NAMED_KEYS.has(key)) return key;
  return undefined;
}

export function eventToShortcut(event: KeyboardEvent): string | undefined {
  const key = normalizeKey(event.key);
  if (!key) return undefined;
  const parts: string[] = [];
  if (event.metaKey || event.ctrlKey) parts.push("Mod");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (parts.length === 0 && !STANDALONE_KEYS.has(key) && !isFunctionKey(key)) {
    return undefined;
  }
  // Mayús no basta para convertir una letra en atajo: eso es escribir.
  if (parts.length === 1 && parts[0] === "Shift" && !STANDALONE_KEYS.has(key)) {
    return undefined;
  }
  return [...parts, key].join("+");
}

export function matchesShortcut(event: KeyboardEvent, shortcut: string): boolean {
  return eventToShortcut(event) === shortcut;
}

/**
 * Réplica exacta de `is_valid_shortcut` de `src-tauri/src/db/organization.rs`.
 * Las acciones de navegación viajan a SQLite, y guardar algo que el backend
 * rechaza convierte un ajuste en un error de guardado.
 */
export function isBackendShortcut(shortcut: string): boolean {
  if (!shortcut || shortcut.length > 64) return false;
  const parts = shortcut.split("+");
  const key = parts.at(-1);
  if (!key) return false;
  const modifiers = parts.slice(0, -1);
  const modifiersValid =
    modifiers.every((part) => part === "Mod" || part === "Alt" || part === "Shift") &&
    new Set(modifiers).size === modifiers.length;
  const keyValid =
    (key.length === 1 && /^[a-z0-9]$/i.test(key)) ||
    BACKEND_NAMED_KEYS.has(key) ||
    isFunctionKey(key);
  return (
    modifiersValid && keyValid && (modifiers.length > 0 || key === "Escape" || isFunctionKey(key))
  );
}

// ── Validación de reasignaciones ───────────────────────────────────────────

export function findShortcutCollision<Action extends string>(
  bindings: Readonly<Record<Action, string>>,
  action: Action,
  shortcut: string,
): Action | undefined {
  return (Object.keys(bindings) as Action[]).find(
    (candidate) => candidate !== action && bindings[candidate] === shortcut,
  );
}

export function getReservedShortcutName(shortcut: string): string | undefined {
  return RESERVED_SHORTCUTS[shortcut];
}

/**
 * Motivo por el que una combinación no puede asignarse a una acción, o
 * `undefined` si es válida. Centraliza las tres reglas —reservado, colisión y
 * compatibilidad con el backend— para que Ajustes no las reimplemente.
 */
export function describeShortcutRejection(
  bindings: ShortcutMap,
  action: AnyShortcutAction,
  shortcut: string,
): string | undefined {
  const reserved = getReservedShortcutName(shortcut);
  if (reserved) return `${shortcutLabel(shortcut)} está reservado para ${reserved}.`;
  const collision = findShortcutCollision(bindings, action, shortcut);
  if (collision) {
    return `${shortcutLabel(shortcut)} ya está asignado a ${shortcutName(collision)}.`;
  }
  const descriptor = SHORTCUT_CATALOGUE.find((entry) => entry.action === action);
  if (descriptor?.persistence === "backend" && !isBackendShortcut(shortcut)) {
    return `${shortcutLabel(shortcut)} no puede guardarse para una acción de navegación: usa una combinación con Mod, Alt o Mayús.`;
  }
  return undefined;
}

// ── Objetivos que nunca deben capturar un atajo ────────────────────────────

export function isEditableShortcutTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.getAttribute("contenteditable") === "true" ||
    ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
    Boolean(target.closest("[contenteditable='true']"))
  );
}

/**
 * Marca la superficie de la biblioteca. Los atajos sin modificador —`Intro`,
 * `Espacio`, flechas, `Inicio`, `Fin`— sólo se disparan dentro de ella: fuera
 * pertenecen al control que tenga el foco, y robárselos rompería cualquier
 * menú, diálogo o botón de la aplicación.
 */
export const LIBRARY_SURFACE_ATTRIBUTE = "data-library-surface";
const LIBRARY_SURFACE_SELECTOR = `[${LIBRARY_SURFACE_ATTRIBUTE}], .game-browser`;

export function isLibrarySurfaceTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(target.closest(LIBRARY_SURFACE_SELECTOR));
}

/** Un atajo sin modificador propio (o sólo con Mayús) necesita esa superficie. */
export function requiresLibrarySurface(shortcut: string): boolean {
  const parts = shortcut.split("+");
  const modifiers = parts.slice(0, -1);
  return modifiers.every((part) => part === "Shift");
}

/** Copiar con el teclado nunca debe pisar una selección de texto real. */
export function hasTextSelection(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean(window.getSelection()?.toString().trim());
}

// ── Persistencia local de las acciones operativas ──────────────────────────

const LOCAL_STORAGE_KEY = "vindexa:shortcuts:v1";

export const SHORTCUTS_CHANGED_EVENT = "vindexa:shortcuts-changed";

export function readLocalShortcuts(): Partial<Record<LocalShortcutAction, string>> {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage?.getItem(LOCAL_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const entries = Object.entries(parsed as Record<string, unknown>).filter(
      (entry): entry is [LocalShortcutAction, string] =>
        typeof entry[1] === "string" &&
        (LOCAL_SHORTCUT_ACTIONS as readonly string[]).includes(entry[0]),
    );
    return Object.fromEntries(entries);
  } catch {
    return {};
  }
}

export function writeLocalShortcuts(overrides: Partial<Record<LocalShortcutAction, string>>): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage?.setItem(LOCAL_STORAGE_KEY, JSON.stringify(overrides));
  } catch {
    // Un almacenamiento lleno o bloqueado no puede tumbar los atajos en memoria.
  }
  window.dispatchEvent(new CustomEvent(SHORTCUTS_CHANGED_EVENT));
}

/**
 * Fusiona lo que viene de SQLite con lo guardado en el navegador y migra el
 * `Mod+K` heredado de la búsqueda. Si una acción local choca con una de
 * navegación —porque el usuario reasignó la segunda— gana la de navegación y la
 * local se queda sin combinación en vez de disparar dos cosas a la vez.
 */
export function resolveShortcuts(
  backend?: ShortcutBindings | undefined,
  local?: Partial<Record<LocalShortcutAction, string>> | undefined,
): ShortcutMap {
  const navigation: ShortcutBindings = { ...DEFAULT_SHORTCUTS, ...backend };
  if (navigation.search === LEGACY_SEARCH_SHORTCUT) navigation.search = DEFAULT_SHORTCUTS.search;
  const taken = new Set<string>();
  const resolved: Record<string, string> = {};
  // La misma regla que abajo, también entre las de navegación: si dos guardadas
  // comparten combinación —Ajustes lo rechaza al grabar, pero una base vieja
  // puede traerlo—, se queda la primera del catálogo y la otra sin asignar, en
  // vez de que una de las dos deje de responder sin decir por qué.
  for (const action of SHORTCUT_ACTIONS) {
    const shortcut = navigation[action];
    if (!shortcut || taken.has(shortcut)) {
      resolved[action] = "";
      continue;
    }
    taken.add(shortcut);
    resolved[action] = shortcut;
  }
  const library = { ...DEFAULT_LOCAL_SHORTCUTS, ...local };
  for (const action of LOCAL_SHORTCUT_ACTIONS) {
    const shortcut = library[action];
    // La cadena vacía significa «sin asignar»: no coincide con ningún evento y
    // Ajustes la presenta como tal, en vez de disparar dos acciones a la vez.
    if (!shortcut || taken.has(shortcut)) {
      resolved[action] = "";
      continue;
    }
    taken.add(shortcut);
    resolved[action] = shortcut;
  }
  return resolved as ShortcutMap;
}

/**
 * Acciones que siguen funcionando con el cursor dentro de un campo de texto.
 *
 * # Por qué existe la lista
 *
 * Escribiendo en el buscador, **ningún** atajo llegaba: se escribía la
 * búsqueda y para ir a Deseados había que soltar el teclado y coger el ratón.
 * Eso es justo lo que los modificadores existen para evitar.
 *
 * # Por qué sólo con un número
 *
 * Dentro de un campo, casi toda combinación con modificador **ya significa
 * algo**: `Mod+A` selecciona el texto, `Mod+C` lo copia, `Mod+Z` deshace, y en
 * macOS `Ctrl+K` corta hasta el final de la línea —y `Ctrl` cuenta como `Mod`
 * aquí—. Secuestrar cualquiera de ésas rompería algo que ya funciona.
 *
 * Un dígito no es ninguna de ellas. Por eso pasa `Mod+1`…`Mod+9` y nada más:
 * resuelve la fricción real —cambiar de sección sin soltar el teclado— sin
 * tocar un solo gesto de edición.
 */
const DISPONIBLES_ESCRIBIENDO: readonly AnyShortcutAction[] = [
  "library",
  "planner",
  "wishlist",
  "collections",
  "tracking",
];

/** Combinación de modificador y un dígito, que dentro de un campo no es nada. */
const MOD_Y_DIGITO = /^(Mod\+|Alt\+|Shift\+)+[0-9]$/;

/** ¿Este atajo puede dispararse aunque se esté escribiendo? */
export function worksWhileTyping(descriptor: ShortcutDescriptor, shortcut: string): boolean {
  if (!DISPONIBLES_ESCRIBIENDO.includes(descriptor.action)) return false;
  return MOD_Y_DIGITO.test(shortcut);
}

/** Primera acción cuyo atajo coincide con el evento, en orden de catálogo. */
export function resolveShortcutEvent(
  event: KeyboardEvent,
  bindings: ShortcutMap,
): ShortcutDescriptor | undefined {
  const shortcut = eventToShortcut(event);
  if (!shortcut) return undefined;
  return SHORTCUT_CATALOGUE.find((entry) => bindings[entry.action] === shortcut);
}

// ── Etiquetas ──────────────────────────────────────────────────────────────

const APPLE_SYMBOLS: Readonly<Record<string, string>> = {
  Mod: "⌘",
  Alt: "⌥",
  Shift: "⇧",
};
/** Orden canónico de Apple: ⌃ ⌥ ⇧ ⌘. */
const APPLE_MODIFIER_ORDER = ["⌃", "⌥", "⇧", "⌘"];

const KEY_LABELS: Readonly<Record<string, string>> = {
  Escape: "Esc",
  Comma: ",",
  Slash: "/",
  Space: "Espacio",
  Enter: "Intro",
  Delete: "Supr",
  Backspace: "Retroceso",
  Insert: "Insert",
  Home: "Inicio",
  End: "Fin",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

export function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Macintosh|Mac OS X|iPhone|iPad/.test(navigator.userAgent);
}

/**
 * Notación de la plataforma: `⇧⌘C` en macOS —con los modificadores en el orden
 * canónico de Apple— y `Ctrl+Shift+C` en el resto.
 */
export function shortcutLabel(shortcut: string, isMacOS = isApplePlatform()): string {
  const parts = shortcut.split("+");
  const key = parts.at(-1) ?? shortcut;
  const modifiers = parts.slice(0, -1);
  const keyLabel = KEY_LABELS[key] ?? key;
  if (!isMacOS) {
    const labels: Readonly<Record<string, string>> = { Mod: "Ctrl", Alt: "Alt", Shift: "Mayús" };
    return [...modifiers.map((part) => labels[part] ?? part), keyLabel].join("+");
  }
  const symbols = modifiers
    .map((part) => APPLE_SYMBOLS[part] ?? part)
    .sort(
      (left, right) => APPLE_MODIFIER_ORDER.indexOf(left) - APPLE_MODIFIER_ORDER.indexOf(right),
    );
  return [...symbols, keyLabel].join("");
}

/**
 * Traduce el mapa vigente al vocabulario de `GameContextMenu`, para que el menú
 * contextual anuncie exactamente las combinaciones que están enlazadas.
 */
export function gameContextShortcuts(
  bindings: ShortcutMap,
): Record<
  | "play"
  | "install"
  | "detail"
  | "store"
  | "reveal"
  | "pin"
  | "tracking"
  | "copyTitle"
  | "copyAppId",
  string
> {
  return {
    play: bindings.primaryAction,
    install: bindings.primaryAction,
    detail: bindings.openDetail,
    store: bindings.openStore,
    reveal: bindings.revealInstallation,
    pin: bindings.togglePinned,
    tracking: bindings.toggleTracking,
    copyTitle: bindings.copyTitle,
    copyAppId: bindings.copyAppId,
  };
}

// ── Contrato entre el shell y la biblioteca ────────────────────────────────

export const FOCUS_SEARCH_EVENT = "vindexa:focus-search";
export const CLOSE_PANEL_EVENT = "vindexa:close-panel";
/** La biblioteca publica aquí el juego enfocado y su contexto. */
export const LIBRARY_CONTEXT_EVENT = "vindexa:library-context";
/** El shell pide una republicación cuando se monta o abre la paleta. */
export const LIBRARY_CONTEXT_REQUEST_EVENT = "vindexa:library-context-request";
/** El shell ordena una operación sobre la biblioteca o el juego enfocado. */
export const LIBRARY_COMMAND_EVENT = "vindexa:library-command";

export interface LibraryContextSnapshot {
  /** Página cargada y visible; nunca el catálogo entero de SQLite. */
  games: readonly GameSummary[];
  /** Juego con el cursor de teclado, si lo hay. */
  focusedAppId?: number | undefined;
  selectedAppIds: readonly number[];
  statuses: readonly StatusDefinition[];
  collections: readonly CollectionSummary[];
  /** Colecciones manuales a las que pertenece cada juego cargado. */
  collectionIdsByApp?: ReadonlyMap<number, readonly string[]> | undefined;
  /** Columnas del plan, para poder elegir destino sin cambiar de pantalla. */
  plannerColumns?: readonly { id: string; name: string }[] | undefined;
  view: LibraryView;
  scopeLabel: string;
}

export type FocusDirection = "up" | "down" | "left" | "right" | "first" | "last";

export type LibraryCommand =
  | { kind: "primary"; appId: number }
  | { kind: "openDetail"; appId: number }
  | { kind: "play"; appId: number }
  | { kind: "install"; appId: number }
  | { kind: "uninstall"; appId: number }
  | { kind: "openStore"; appId: number }
  | { kind: "reveal"; appId: number }
  | { kind: "setStatus"; appId: number; statusId: string }
  | { kind: "cycleStatus"; appId: number; direction: 1 | -1 }
  | { kind: "setPriority"; appId: number; priority: number }
  | { kind: "shiftPriority"; appId: number; direction: 1 | -1 }
  | { kind: "togglePinned"; appId: number }
  | { kind: "toggleTracking"; appId: number }
  | { kind: "toggleCollection"; appId: number; collectionId: string }
  /** `columnId` ausente significa la primera columna del plan. */
  | { kind: "addToPlanner"; appIds: readonly number[]; columnId?: string }
  | { kind: "copyTitle"; appId: number }
  | { kind: "copyAppId"; appId: number }
  | { kind: "focus"; appId: number }
  | { kind: "moveFocus"; direction: FocusDirection; extend: boolean }
  | { kind: "undo" }
  | { kind: "selectAll" }
  | { kind: "setView"; view: LibraryView };

export function dispatchLibraryCommand(command: LibraryCommand): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(LIBRARY_COMMAND_EVENT, { detail: command }));
}

export function publishLibraryContext(snapshot: LibraryContextSnapshot): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(LIBRARY_CONTEXT_EVENT, { detail: snapshot }));
}

export function requestLibraryContext(): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(LIBRARY_CONTEXT_REQUEST_EVENT));
}

function subscribe<Detail>(type: string, handler: (detail: Detail) => void): () => void {
  const listener = (event: Event) => handler((event as CustomEvent<Detail>).detail);
  window.addEventListener(type, listener);
  return () => window.removeEventListener(type, listener);
}

/** La usa el shell: escucha lo que publica la biblioteca. */
export function onLibraryContext(handler: (snapshot: LibraryContextSnapshot) => void): () => void {
  return subscribe<LibraryContextSnapshot>(LIBRARY_CONTEXT_EVENT, handler);
}

/** La usa la biblioteca: escucha las órdenes del shell y de la paleta. */
export function onLibraryCommand(handler: (command: LibraryCommand) => void): () => void {
  return subscribe<LibraryCommand>(LIBRARY_COMMAND_EVENT, handler);
}

/** La usa la biblioteca: responde republicando su contexto. */
export function onLibraryContextRequest(handler: () => void): () => void {
  const listener = () => handler();
  window.addEventListener(LIBRARY_CONTEXT_REQUEST_EVENT, listener);
  return () => window.removeEventListener(LIBRARY_CONTEXT_REQUEST_EVENT, listener);
}
