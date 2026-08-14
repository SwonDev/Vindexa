import type { ShortcutAction, ShortcutBindings } from "@/lib/types";

export const DEFAULT_SHORTCUTS: ShortcutBindings = {
  library: "Mod+1",
  planner: "Mod+2",
  collections: "Mod+3",
  tracking: "Mod+4",
  search: "Mod+K",
  sync: "Mod+Shift+S",
  closePanel: "Escape",
};

export const SHORTCUT_ACTIONS: readonly ShortcutAction[] = [
  "library",
  "planner",
  "collections",
  "tracking",
  "search",
  "sync",
  "closePanel",
];

const RESERVED_SHORTCUTS: Readonly<Record<string, string>> = {
  "Mod+Comma": "Abrir Ajustes",
};

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift"]);

function normalizeKey(key: string): string | undefined {
  if (MODIFIER_KEYS.has(key) || key === "Dead" || key === "Unidentified") return undefined;
  if (key.length === 1 && /[a-z0-9]/i.test(key)) return key.toUpperCase();
  if (key === " ") return "Space";
  if (key === ",") return "Comma";
  if (key === "/") return "Slash";
  if (/^F(?:[1-9]|1[0-2])$/.test(key)) return key;
  if (
    [
      "Escape",
      "Enter",
      "Backspace",
      "Delete",
      "ArrowUp",
      "ArrowDown",
      "ArrowLeft",
      "ArrowRight",
    ].includes(key)
  ) {
    return key;
  }
  return undefined;
}

export function eventToShortcut(event: KeyboardEvent): string | undefined {
  const key = normalizeKey(event.key);
  if (!key) return undefined;
  const parts: string[] = [];
  if (event.metaKey || event.ctrlKey) parts.push("Mod");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (parts.length === 0 && key !== "Escape" && !/^F(?:[1-9]|1[0-2])$/.test(key)) {
    return undefined;
  }
  return [...parts, key].join("+");
}

export function matchesShortcut(event: KeyboardEvent, shortcut: string): boolean {
  return eventToShortcut(event) === shortcut;
}

export function findShortcutCollision(
  bindings: ShortcutBindings,
  action: ShortcutAction,
  shortcut: string,
): ShortcutAction | undefined {
  return SHORTCUT_ACTIONS.find(
    (candidate) => candidate !== action && bindings[candidate] === shortcut,
  );
}

export function getReservedShortcutName(shortcut: string): string | undefined {
  return RESERVED_SHORTCUTS[shortcut];
}

export function isEditableShortcutTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.getAttribute("contenteditable") === "true" ||
    ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
    Boolean(target.closest("[contenteditable='true']"))
  );
}

export function shortcutLabel(
  shortcut: string,
  isMacOS = /Macintosh|Mac OS X/.test(navigator.userAgent),
) {
  const labels: Record<string, string> = {
    Mod: isMacOS ? "⌘" : "Ctrl",
    Alt: isMacOS ? "⌥" : "Alt",
    Shift: isMacOS ? "⇧" : "Mayús",
    Escape: "Esc",
    Comma: ",",
    Slash: "/",
    Space: "Espacio",
    ArrowUp: "↑",
    ArrowDown: "↓",
    ArrowLeft: "←",
    ArrowRight: "→",
  };
  return shortcut
    .split("+")
    .map((part) => labels[part] ?? part)
    .join(isMacOS ? "" : "+");
}
