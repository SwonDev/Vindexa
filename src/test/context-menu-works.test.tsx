import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { installWebviewChromeGuards } from "@/lib/webview-chrome";

/**
 * El clic derecho tiene que abrir el menú de Vindexa.
 *
 * # Por qué esta prueba existe
 *
 * Las guardas del cromado cancelan el menú nativo de WebKit interceptando
 * `contextmenu` en captura, para que el clic derecho no ofrezca «Recargar» ni
 * «Inspeccionar» en una aplicación de escritorio. Esa intercepción convive con
 * los menús propios —los de colección, estado, tienda y juego—, y esa
 * convivencia es exactamente lo que aquí se comprueba: que cancelar el nativo
 * no se lleva por delante el nuestro.
 *
 * Sin esto, un cambio en las guardas deja la aplicación entera sin clic derecho
 * y nada falla al compilar.
 */

let quitarGuardas: (() => void) | undefined;

afterEach(() => {
  quitarGuardas?.();
  quitarGuardas = undefined;
});

function Menu() {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <button type="button">Una colección</button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem>Cambiar color</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

describe("el clic derecho abre el menú de Vindexa", () => {
  it("con las guardas del cromado instaladas", async () => {
    quitarGuardas = installWebviewChromeGuards({ allowNativeMenuWithShift: false });
    const user = userEvent.setup();
    render(<Menu />);

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: "Una colección" }),
    });

    await waitFor(() =>
      expect(screen.getByRole("menuitem", { name: "Cambiar color" })).toBeVisible(),
    );
  });

  it("y también sin ellas, para saber cuál de las dos cosas falla", async () => {
    const user = userEvent.setup();
    render(<Menu />);

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: "Una colección" }),
    });

    await waitFor(() =>
      expect(screen.getByRole("menuitem", { name: "Cambiar color" })).toBeVisible(),
    );
  });

  it("y el menú nativo de WebKit se sigue cancelando donde no hay menú propio", async () => {
    // Es la otra mitad: si por arreglar lo nuestro dejáramos pasar el menú de
    // WebKit, el clic derecho en cualquier hueco ofrecería «Recargar» e
    // «Inspeccionar» en una aplicación de escritorio.
    quitarGuardas = installWebviewChromeGuards({ allowNativeMenuWithShift: false });
    render(
      <div>
        <p>Un hueco cualquiera</p>
      </div>,
    );

    const evento = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    screen.getByText("Un hueco cualquiera").dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(true);
  });
});
