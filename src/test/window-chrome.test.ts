import { describe, expect, it } from "vitest";
import { isEmptyChromeTarget } from "@/features/shell/window-chrome";

/**
 * Qué cuenta como «parte vacía» de la barra.
 *
 * El gesto de la barra de título es doble clic sobre el cromado, no sobre lo
 * que hay encima. Quien pulsa dos veces seguidas en «Sincronizar» no está
 * pidiendo que la ventana cambie de tamaño, y confundir las dos cosas convierte
 * un acierto en un salto de ventana cada vez que alguien insiste en un botón.
 */
describe("doble clic en la barra de la ventana", () => {
  function element(html: string): Element {
    const host = document.createElement("div");
    host.innerHTML = html;
    return host.firstElementChild as Element;
  }

  it("el fondo de la barra y sus rótulos sí cuentan", () => {
    expect(isEmptyChromeTarget(element("<header class='topbar'></header>"))).toBe(true);
    expect(isEmptyChromeTarget(element("<span>VINDEXA</span>"))).toBe(true);
  });

  it("lo que se puede pulsar o escribir no cuenta", () => {
    const casos = [
      "<button type='button'>Sincronizar</button>",
      "<a href='#'>Enlace</a>",
      "<input value='texto' />",
      "<div role='button'>Pestaña</div>",
      "<div role='tab'>Biblioteca</div>",
      "<textarea></textarea>",
    ];
    for (const html of casos) {
      expect(isEmptyChromeTarget(element(html)), html).toBe(false);
    }
  });

  it("un icono dentro de un botón tampoco cuenta", () => {
    // El objetivo real de un clic suele ser el `svg` de dentro, no el botón:
    // mirar sólo el elemento pulsado dejaba pasar justo el caso más frecuente.
    const boton = element("<button type='button'><svg aria-hidden='true'></svg> Ajustes</button>");
    expect(isEmptyChromeTarget(boton.querySelector("svg"))).toBe(false);
  });

  it("sin objetivo no se hace nada", () => {
    expect(isEmptyChromeTarget(null)).toBe(false);
  });
});
