import { describe, expect, it, vi } from "vitest";
import { handleTitleBarPointerDown, isWindowDragArea } from "@/features/shell/window-chrome";

function mouseEvent(target: EventTarget | null, detail: number, button = 0) {
  return { button, detail, target, preventDefault: vi.fn() };
}

describe("gestos de la barra de título", () => {
  it("trata como zona de ventana el espacio inerte de la barra", () => {
    document.body.innerHTML = `
      <header class="topbar">
        <div class="brand"><span>VINDEXA</span></div>
        <button type="button">Ajustes</button>
      </header>`;
    const blank = document.querySelector(".brand span");
    const control = document.querySelector("button");

    expect(isWindowDragArea(blank)).toBe(true);
    expect(isWindowDragArea(control)).toBe(false);
    expect(isWindowDragArea(null)).toBe(false);
  });

  it("maximiza con el segundo clic de un doble clic sobre el espacio vacío", () => {
    document.body.innerHTML = `<header class="topbar"><div class="brand"></div></header>`;
    const blank = document.querySelector(".brand");

    expect(handleTitleBarPointerDown(mouseEvent(blank, 1))).toBe("drag");
    expect(handleTitleBarPointerDown(mouseEvent(blank, 2))).toBe("maximize");
  });

  it("no secuestra el gesto sobre controles ni con el botón secundario", () => {
    document.body.innerHTML = `<header class="topbar"><button type="button">Ajustes</button></header>`;
    const control = document.querySelector("button");
    const onControl = mouseEvent(control, 2);
    const secondary = mouseEvent(control, 1, 2);

    expect(handleTitleBarPointerDown(onControl)).toBe("ignore");
    expect(onControl.preventDefault).not.toHaveBeenCalled();
    expect(handleTitleBarPointerDown(secondary)).toBe("ignore");
  });
});
