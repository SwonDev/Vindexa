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
 * Las guardas del cromado, contra lo que cada una podría romper.
 *
 * # Por qué existe este archivo
 *
 * `installWebviewChromeGuards` instala cinco manejadores globales que cancelan
 * comportamientos nativos del webview: menú contextual, selección de texto,
 * arrastre de imágenes, zoom y «swipe atrás». Cada uno convive con una función
 * de Vindexa que hace algo parecido, y **una de esas convivencias estuvo rota
 * durante toda la vida del archivo**: la guarda del menú contextual escuchaba
 * en captura y dejaba sin clic derecho a la aplicación entera, con un comentario
 * al lado afirmando que no interfería.
 *
 * Nada de aquello fallaba al compilar, ni en las pruebas de los componentes por
 * separado, ni en las pruebas de las propias guardas. Sólo se veía usando la
 * aplicación, porque el fallo **no está en ninguna de las dos piezas: está en
 * que se pisan**.
 *
 * De ahí que estas pruebas monten las dos cosas a la vez. No comprueban que un
 * componente exista: comprueban que sigue funcionando con las guardas puestas.
 */

let quitarGuardas: (() => void) | undefined;

function conGuardas() {
  quitarGuardas = installWebviewChromeGuards({ allowNativeMenuWithShift: false });
}

afterEach(() => {
  quitarGuardas?.();
  quitarGuardas = undefined;
});

describe("guarda del menú contextual", () => {
  it("no impide los menús propios de Vindexa", async () => {
    conGuardas();
    const user = userEvent.setup();
    render(
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <button type="button">Una colección</button>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem>Cambiar color</ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>,
    );

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: "Una colección" }),
    });
    await waitFor(() =>
      expect(screen.getByRole("menuitem", { name: "Cambiar color" })).toBeVisible(),
    );
  });
});

describe("guarda de la selección de texto", () => {
  it("no impide escribir ni seleccionar en un campo", async () => {
    // Cancelar `selectstart` en un campo dejaría un formulario donde no se puede
    // corregir lo escrito.
    conGuardas();
    const user = userEvent.setup();
    render(<input aria-label="Buscar" defaultValue="" />);

    const campo = screen.getByLabelText("Buscar");
    await user.type(campo, "hollow");
    expect(campo).toHaveValue("hollow");

    const evento = new Event("selectstart", { bubbles: true, cancelable: true });
    campo.dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(false);
  });

  it("sí la impide en una superficie que no es texto", () => {
    // Ahí es donde molesta: arrastrar sobre una rejilla de carátulas y acabar
    // con media pantalla seleccionada en azul.
    conGuardas();
    render(<div data-testid="rejilla">carátulas</div>);

    const evento = new Event("selectstart", { bubbles: true, cancelable: true });
    screen.getByTestId("rejilla").dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(true);
  });
});

describe("guarda del arrastre nativo", () => {
  it("cancela el fantasma de una imagen", () => {
    conGuardas();
    render(<img src="portada.jpg" alt="Portada" />);

    const evento = new Event("dragstart", { bubbles: true, cancelable: true });
    screen.getByAltText("Portada").dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(true);
  });

  it("no toca el arrastre de la aplicación, que no es nativo", () => {
    // dnd-kit trabaja con eventos de puntero: un `dragstart` cancelado no le
    // afecta. Se comprueba que un elemento cualquiera —una tarjeta— no queda
    // marcado como cancelado, porque si algún día se cancelara en todo, el
    // arrastre nativo de un enlace legítimo también moriría sin aviso.
    conGuardas();
    render(<article data-testid="tarjeta">Un juego</article>);

    const evento = new Event("dragstart", { bubbles: true, cancelable: true });
    screen.getByTestId("tarjeta").dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(false);
  });
});

describe("guarda del gesto «atrás» del trackpad", () => {
  /** Un contenedor que de verdad se puede desplazar en horizontal. */
  function contenedorDesplazable(scrollLeft: number, scrollWidth = 1000, clientWidth = 400) {
    const { getByTestId } = render(
      <div data-testid="carril" style={{ overflowX: "auto" }}>
        <div>columnas</div>
      </div>,
    );
    const carril = getByTestId("carril");
    Object.defineProperty(carril, "scrollWidth", { value: scrollWidth, configurable: true });
    Object.defineProperty(carril, "clientWidth", { value: clientWidth, configurable: true });
    carril.scrollLeft = scrollLeft;
    return carril;
  }

  it("deja desplazar el planificador cuando queda sitio", () => {
    // Si esto se cancelara, el planificador no se podría recorrer con el
    // trackpad y parecería que la aplicación ignora el gesto.
    conGuardas();
    const carril = contenedorDesplazable(0);

    const evento = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaX: 40,
      deltaY: 0,
    });
    carril.dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(false);
  });

  it("corta el gesto cuando ya no queda sitio, que es cuando sacaría de la aplicación", () => {
    conGuardas();
    // Desplazado hasta el final: 1000 - 400 = 600.
    const carril = contenedorDesplazable(600);

    const evento = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaX: 40,
      deltaY: 0,
    });
    carril.dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(true);
  });

  it("no toca el desplazamiento vertical", () => {
    conGuardas();
    render(<div data-testid="lista">juegos</div>);

    const evento = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaX: 0,
      deltaY: 120,
    });
    screen.getByTestId("lista").dispatchEvent(evento);
    expect(evento.defaultPrevented).toBe(false);
  });
});

describe("guarda del zoom por teclado", () => {
  it("no se come los atajos de Vindexa", () => {
    // `Mod+1`…`Mod+4` cambian de sección y se pueden reasignar: bloquearlos
    // aquí dejaría a alguien sin navegación sin saber por qué.
    conGuardas();
    render(<div data-testid="fondo">biblioteca</div>);

    for (const key of ["1", "2", "3", "4", "0", "k", ","]) {
      const evento = new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        key,
        metaKey: true,
      });
      screen.getByTestId("fondo").dispatchEvent(evento);
      expect(evento.defaultPrevented, `Mod+${key}`).toBe(false);
    }
  });

  it("sí bloquea el zoom del webview", () => {
    conGuardas();
    render(<div data-testid="fondo">biblioteca</div>);

    for (const key of ["+", "-", "=", "_"]) {
      const evento = new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        key,
        metaKey: true,
      });
      screen.getByTestId("fondo").dispatchEvent(evento);
      expect(evento.defaultPrevented, `Mod+${key}`).toBe(true);
    }
  });
});
