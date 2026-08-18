import { afterEach, describe, expect, it, vi } from "vitest";
import {
  hasTextSelectionAt,
  installWebviewChromeGuards,
  isEditableTarget,
  isSelectableTarget,
} from "@/lib/webview-chrome";

let uninstall: (() => void) | undefined;

function install(options: Parameters<typeof installWebviewChromeGuards>[0] = {}) {
  uninstall = installWebviewChromeGuards({ allowNativeMenuWithShift: false, ...options });
  return uninstall;
}

function mount(html: string): HTMLElement {
  const host = document.createElement("div");
  host.innerHTML = html;
  document.body.append(host);
  return host;
}

function dispatchContextMenu(target: Element, init: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

afterEach(() => {
  uninstall?.();
  uninstall = undefined;
  document.body.innerHTML = "";
});

describe("guardas del cromo de webview", () => {
  it("bloquea el menú contextual nativo en superficies no textuales", () => {
    install();
    const host = mount('<article class="game-card"><h3>Portal 2</h3></article>');
    const card = host.querySelector("article") as HTMLElement;

    expect(dispatchContextMenu(card).defaultPrevented).toBe(true);
  });

  it("respeta el menú nativo en campos de texto editables", () => {
    install();
    const host = mount(
      '<div><input aria-label="Buscar" /><textarea aria-label="Notas"></textarea>' +
        '<div contenteditable="true">Diario</div><input type="checkbox" /></div>',
    );

    const input = host.querySelector("input:not([type])") as HTMLInputElement;
    const textarea = host.querySelector("textarea") as HTMLTextAreaElement;
    const editable = host.querySelector("[contenteditable]") as HTMLElement;
    const checkbox = host.querySelector('input[type="checkbox"]') as HTMLInputElement;

    expect(dispatchContextMenu(input).defaultPrevented).toBe(false);
    expect(dispatchContextMenu(textarea).defaultPrevented).toBe(false);
    expect(dispatchContextMenu(editable).defaultPrevented).toBe(false);
    // Una casilla no es un campo de texto: ahí el menú nativo no aporta nada.
    expect(dispatchContextMenu(checkbox).defaultPrevented).toBe(true);
  });

  it("respeta el menú nativo cuando hay una selección de texto real", () => {
    install();
    const host = mount("<p>Descripción del juego</p>");
    const paragraph = host.querySelector("p") as HTMLElement;

    const selectionSpy = vi.spyOn(document, "getSelection").mockReturnValue({
      isCollapsed: false,
      rangeCount: 1,
      toString: () => "Descripción",
      containsNode: () => true,
    } as unknown as Selection);

    expect(dispatchContextMenu(paragraph).defaultPrevented).toBe(false);
    selectionSpy.mockRestore();
  });

  it("permite el menú nativo con Shift solo cuando la opción está activa", () => {
    install({ allowNativeMenuWithShift: true });
    const host = mount("<div>Panel</div>");
    const panel = host.querySelector("div") as HTMLElement;

    expect(dispatchContextMenu(panel, { shiftKey: true }).defaultPrevented).toBe(false);
    expect(dispatchContextMenu(panel).defaultPrevented).toBe(true);

    uninstall?.();
    install({ allowNativeMenuWithShift: false });
    expect(dispatchContextMenu(panel, { shiftKey: true }).defaultPrevented).toBe(true);
  });

  it("bloquea la selección por arrastre salvo en superficies marcadas", () => {
    install();
    const host = mount(
      '<div><span id="chrome">Biblioteca</span>' +
        '<p data-selectable><span id="prosa">Descripción</span></p>' +
        '<input aria-label="Buscar" /></div>',
    );

    const fire = (target: Element) => {
      const event = new Event("selectstart", { bubbles: true, cancelable: true });
      target.dispatchEvent(event);
      return event.defaultPrevented;
    };

    expect(fire(host.querySelector("#chrome") as HTMLElement)).toBe(true);
    expect(fire(host.querySelector("#prosa") as HTMLElement)).toBe(false);
    expect(fire(host.querySelector("input") as HTMLElement)).toBe(false);
  });

  it("bloquea el arrastre nativo de imágenes y enlaces", () => {
    install();
    const host = mount(
      '<div><img alt="Portada" src="cover.png" />' +
        '<a href="https://store.steampowered.com">Tienda</a>' +
        '<img alt="Libre" src="free.png" data-allow-native-drag="true" />' +
        "<span>Texto</span></div>",
    );

    const fire = (target: Element) => {
      const event = new Event("dragstart", { bubbles: true, cancelable: true });
      target.dispatchEvent(event);
      return event.defaultPrevented;
    };

    expect(fire(host.querySelector('img[alt="Portada"]') as HTMLElement)).toBe(true);
    expect(fire(host.querySelector("a") as HTMLElement)).toBe(true);
    expect(fire(host.querySelector('img[alt="Libre"]') as HTMLElement)).toBe(false);
    expect(fire(host.querySelector("span") as HTMLElement)).toBe(false);
  });

  it("bloquea el zoom por rueda y por teclado del webview", () => {
    install();
    const host = mount("<div>Contenido</div>");
    const panel = host.querySelector("div") as HTMLElement;

    const wheelZoom = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      deltaY: -120,
    });
    panel.dispatchEvent(wheelZoom);
    expect(wheelZoom.defaultPrevented).toBe(true);

    const wheelScroll = new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -120 });
    panel.dispatchEvent(wheelScroll);
    expect(wheelScroll.defaultPrevented).toBe(false);

    const zoomIn = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "+",
      metaKey: true,
    });
    panel.dispatchEvent(zoomIn);
    expect(zoomIn.defaultPrevented).toBe(true);

    // Los atajos de Vindexa con dígitos deben seguir llegando a la aplicación.
    const appShortcut = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "1",
      metaKey: true,
    });
    panel.dispatchEvent(appShortcut);
    expect(appShortcut.defaultPrevented).toBe(false);
  });

  it("bloquea el sobredesplazamiento horizontal que dispara el swipe atrás", () => {
    install();
    const host = mount("<div>Rejilla</div>");
    const grid = host.querySelector("div") as HTMLElement;

    const swipeBack = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaX: -140,
      deltaY: 0,
    });
    grid.dispatchEvent(swipeBack);

    expect(swipeBack.defaultPrevented).toBe(true);
    expect(document.documentElement.style.getPropertyValue("overscroll-behavior-x")).toBe("none");
  });

  it("desinstala todas las guardas y deja el documento como estaba", () => {
    install();
    const host = mount("<div>Panel</div>");
    const panel = host.querySelector("div") as HTMLElement;
    expect(dispatchContextMenu(panel).defaultPrevented).toBe(true);

    uninstall?.();
    uninstall = undefined;

    expect(dispatchContextMenu(panel).defaultPrevented).toBe(false);
    expect(document.documentElement.style.getPropertyValue("overscroll-behavior-x")).toBe("");

    const drag = new Event("dragstart", { bubbles: true, cancelable: true });
    (host.querySelector("div") as HTMLElement).dispatchEvent(drag);
    expect(drag.defaultPrevented).toBe(false);
  });

  it("no falla sin documento disponible", () => {
    const noop = installWebviewChromeGuards({ document: undefined as unknown as Document });
    expect(typeof noop).toBe("function");
    expect(() => noop()).not.toThrow();
  });
});

describe("clasificación de objetivos", () => {
  it("reconoce campos editables y superficies seleccionables", () => {
    const host = mount(
      '<div><input aria-label="Buscar" /><span id="plano">Texto</span>' +
        '<p data-selectable id="prosa">Descripción</p></div>',
    );

    expect(isEditableTarget(host.querySelector("input"))).toBe(true);
    expect(isEditableTarget(host.querySelector("#plano"))).toBe(false);
    expect(isSelectableTarget(host.querySelector("#prosa"))).toBe(true);
    expect(isSelectableTarget(host.querySelector("#plano"))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });

  it("ignora selecciones vacías o colapsadas", () => {
    const host = mount("<p>Descripción</p>");
    const paragraph = host.querySelector("p") as HTMLElement;

    const spy = vi.spyOn(document, "getSelection").mockReturnValue({
      isCollapsed: true,
      rangeCount: 1,
      toString: () => "",
    } as unknown as Selection);

    expect(hasTextSelectionAt(paragraph, document)).toBe(false);
    spy.mockRestore();
  });
});
