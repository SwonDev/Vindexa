import { describe, expect, it } from "vitest";
import {
  DEFAULT_SHORTCUTS,
  eventToShortcut,
  findShortcutCollision,
  getReservedShortcutName,
  isEditableShortcutTarget,
  matchesShortcut,
} from "@/features/shell/shortcuts";

function keyboardEvent(key: string, init: KeyboardEventInit = {}) {
  return new KeyboardEvent("keydown", { key, ...init });
}

describe("atajos configurables", () => {
  it("normaliza Command en macOS y Control en otras plataformas como Mod", () => {
    expect(eventToShortcut(keyboardEvent("k", { metaKey: true }))).toBe("Mod+K");
    expect(eventToShortcut(keyboardEvent("k", { ctrlKey: true }))).toBe("Mod+K");
    expect(eventToShortcut(keyboardEvent("S", { ctrlKey: true, shiftKey: true }))).toBe(
      "Mod+Shift+S",
    );
  });

  it("detecta colisiones antes de persistir una asignación", () => {
    expect(findShortcutCollision(DEFAULT_SHORTCUTS, "planner", "Mod+1")).toBe("library");
    expect(findShortcutCollision(DEFAULT_SHORTCUTS, "planner", "Mod+2")).toBeUndefined();
    expect(getReservedShortcutName("Mod+Comma")).toBe("Abrir Ajustes");
    expect(getReservedShortcutName("Mod+K")).toBeUndefined();
  });

  it("no activa atajos mientras la persona escribe", () => {
    const input = document.createElement("input");
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");

    expect(isEditableShortcutTarget(input)).toBe(true);
    expect(isEditableShortcutTarget(editor)).toBe(true);
    expect(isEditableShortcutTarget(document.createElement("button"))).toBe(false);
  });

  it("compara el evento contra la combinación persistida", () => {
    expect(matchesShortcut(keyboardEvent("1", { metaKey: true }), "Mod+1")).toBe(true);
    expect(matchesShortcut(keyboardEvent("1"), "Mod+1")).toBe(false);
    expect(matchesShortcut(keyboardEvent("Escape"), "Escape")).toBe(true);
  });
});
