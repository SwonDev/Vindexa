import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { GamePreviewCard } from "@/components/common/GamePreviewCard";

vi.mock("@/lib/tauri", () => ({
  // Sin capturas: lo que se comprueba aquí es el gesto, no la imagen.
  api: { gamePreview: () => Promise.resolve({ screenshots: [] }) },
  getErrorMessage: () => "error",
}));

/**
 * La vista rápida se va al pulsar.
 *
 * # El fallo
 *
 * El emergente se cierra cuando el puntero **sale** del disparador. Al pulsar
 * una fila se abre la ficha del juego encima, pero el puntero no se ha movido:
 * el navegador no vuelve a mirar qué hay debajo hasta el siguiente movimiento,
 * así que no llega ningún `pointerleave` y la tarjeta se queda flotando sobre
 * la ficha recién abierta. Se ve en cuanto se abre un juego con el ratón.
 *
 * # Lo que no puede romperse al arreglarlo
 *
 * El manejador que abre la ficha vive en las props que llegan de fuera. Cerrar
 * la vista rápida no puede sustituirlo: es exactamente la clase de fallo que
 * dejó la aplicación sin clic derecho —dos piezas que se pisan y ninguna de las
 * dos parece rota—.
 */
describe("la vista rápida al pulsar", () => {
  it("deja pasar el clic sin emergente por medio", async () => {
    const user = userEvent.setup();
    const abrir = vi.fn();
    render(
      <GamePreviewCard appId={620} title="Portal 2" onClick={abrir}>
        <button type="button">Portal 2</button>
      </GamePreviewCard>,
    );
    await user.click(screen.getByRole("button", { name: "Portal 2" }));
    expect(abrir).toHaveBeenCalledTimes(1);
  });

  it("se cierra y deja pasar el clic que abre la ficha", async () => {
    const user = userEvent.setup();
    const abrir = vi.fn();
    render(
      <GamePreviewCard appId={620} title="Portal 2" onClick={abrir}>
        <button type="button">Portal 2</button>
      </GamePreviewCard>,
    );

    const fila = screen.getByRole("button", { name: "Portal 2" });
    await user.hover(fila);
    // El emergente tarda a propósito en aparecer: se espera a que el disparador
    // se declare abierto, que es lo que Radix marca en el propio elemento.
    await waitFor(() => expect(fila).toHaveAttribute("data-state", "open"), { timeout: 3_000 });

    await user.click(fila);

    expect(abrir).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(fila).toHaveAttribute("data-state", "closed"));
  });
});
