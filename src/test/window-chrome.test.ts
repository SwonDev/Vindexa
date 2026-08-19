import { describe, expect, it, vi } from "vitest";
import { fitWindowToAvailableSpace } from "@/features/shell/window-chrome";

/**
 * El arrastre y el doble clic de la barra de título los resuelve el script de
 * arrastre de Tauri —la barra los declara con `data-tauri-drag-region`—, así
 * que lo que queda por comprobar aquí es que la ventana se abre ocupando el
 * hueco real y que fuera del escritorio no intenta hablar con nadie.
 */
describe("ajuste de la ventana al abrirse", () => {
  it("no pide nada cuando no hay contenedor de escritorio", async () => {
    // En las pruebas y en el navegador el módulo de Tauri se importa igual,
    // pero su puente no existe: pedirle algo dejaría la promesa esperando para
    // siempre y con ella el arranque de la interfaz.
    expect("__TAURI_INTERNALS__" in window).toBe(false);
    await expect(fitWindowToAvailableSpace()).resolves.toBeUndefined();
  });

  it("no falla si la pantalla no publica su espacio disponible", async () => {
    const original = Object.getOwnPropertyDescriptor(window, "screen");
    Object.defineProperty(window, "screen", {
      value: { availWidth: 0, availHeight: 0 },
      configurable: true,
    });
    const bridge = vi.fn();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: { invoke: bridge },
      configurable: true,
    });

    await expect(fitWindowToAvailableSpace()).resolves.toBeUndefined();

    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    if (original) Object.defineProperty(window, "screen", original);
  });
});
