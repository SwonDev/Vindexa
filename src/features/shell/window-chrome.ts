/**
 * La ventana: cómo se abre y qué hace el doble clic en la barra.
 *
 * # Por qué el arrastre no vive aquí
 *
 * Vivió, y no funcionaba. Resolver el arrastre a mano obliga a acertar con el
 * orden exacto en que cada sistema entrega los eventos —en macOS, pedir el
 * arrastre al pulsar hace que el segundo clic no llegue nunca— y a mantener esa
 * pieza para siempre. El propio Tauri ya trae ese trabajo hecho y probado en su
 * zona de arrastre, así que la barra lo declara con `data-tauri-drag-region`.
 *
 * # Por qué el doble clic sí
 *
 * Porque lo que hace Tauri por su cuenta no es lo que se pide. En macOS acaba
 * en el *zoom* del sistema, que lleva la ventana al tamaño que la aplicación
 * declara como preferido, no al que cabe en la pantalla: medido, 1440×870 sobre
 * un área útil de 1512×982. Aquí el doble clic hace lo mismo que el arranque
 * —ocupar el hueco real— y, si ya lo ocupa, devuelve la ventana a como estaba.
 */

/**
 * ¿Hay un contenedor de escritorio al otro lado?
 *
 * Comprobarlo antes de pedir nada es lo que evita que una llamada de ventana se
 * quede esperando para siempre una respuesta que nadie va a dar: en las pruebas
 * y en `pnpm dev` el módulo de Tauri se importa igual, pero su puente no
 * existe.
 */
function hasDesktopBridge(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function currentWindow() {
  if (!hasDesktopBridge()) return undefined;
  try {
    const module = await import("@tauri-apps/api/window");
    return module.getCurrentWindow();
  } catch {
    // Fuera del contenedor de escritorio (pruebas, `pnpm dev` en navegador) no
    // hay ventana nativa que manipular y el gesto simplemente no aplica.
    return undefined;
  }
}

/**
 * Abre la ventana ocupando el espacio disponible de la pantalla.
 *
 * No basta con `maximized` en la configuración ni con pedir `maximize()`: en
 * macOS ambas cosas acaban en el *zoom* del sistema, que lleva la ventana al
 * tamaño que la aplicación declara como preferido en vez de al que cabe.
 * Medido: la ventana quedaba en 1440×870 sobre una pantalla útil de 1512×982.
 *
 * Quien sí conoce el hueco exacto es el propio motor web: `screen.avail*`
 * descuenta la barra de menús, el Dock y cualquier otra barra del sistema. Es
 * además la misma cuenta en las tres plataformas.
 *
 * Se ajusta sólo al abrir. Después la ventana es de quien la usa: si la
 * redimensiona, nadie se lo deshace.
 */
export async function fitWindowToAvailableSpace(): Promise<void> {
  const space = availableSpace();
  if (!space) return;
  const appWindow = await currentWindow();
  if (!appWindow) return;
  try {
    const { LogicalPosition, LogicalSize } = await import("@tauri-apps/api/dpi");
    await appWindow.setPosition(new LogicalPosition(space.x, space.y));
    await appWindow.setSize(new LogicalSize(space.width, space.height));
  } catch {
    // Sin ventana nativa —pruebas, navegador— no hay nada que ajustar.
  }
}

/** Geometría anterior, para que el segundo doble clic devuelva la ventana. */
interface Geometry {
  x: number;
  y: number;
  width: number;
  height: number;
}
let restorePoint: Geometry | undefined;

/** Margen al comparar tamaños: el sistema redondea y nunca cuadra al píxel. */
const FIT_TOLERANCE = 8;

/** El hueco real de la pantalla, o `undefined` si el motor no lo dice. */
function availableSpace(): Geometry | undefined {
  if (typeof window === "undefined" || typeof window.screen === "undefined") return undefined;
  const { availWidth, availHeight } = window.screen;
  if (!availWidth || !availHeight) return undefined;
  // `availLeft` y `availTop` no están en todos los motores; su ausencia
  // significa que el área útil empieza en el origen.
  const screen = window.screen as Screen & { availLeft?: number; availTop?: number };
  return {
    x: screen.availLeft ?? 0,
    y: screen.availTop ?? 0,
    width: availWidth,
    height: availHeight,
  };
}

/**
 * Doble clic en la parte vacía de la barra: ocupa el hueco disponible y, si ya
 * lo ocupa, devuelve la ventana al tamaño que tenía antes.
 *
 * Es el gesto que todo el mundo espera de una barra de título, y hacerlo aquí
 * es lo único que garantiza que «maximizar» signifique llenar la pantalla útil
 * en las tres plataformas y no lo que cada sistema entienda por ello.
 */
export async function toggleFitWindow(): Promise<void> {
  const space = availableSpace();
  if (!space) return;
  const appWindow = await currentWindow();
  if (!appWindow) return;
  try {
    const { LogicalPosition, LogicalSize } = await import("@tauri-apps/api/dpi");
    const factor = await appWindow.scaleFactor();
    const size = (await appWindow.innerSize()).toLogical(factor);
    const ocupaElHueco =
      Math.abs(size.width - space.width) <= FIT_TOLERANCE &&
      Math.abs(size.height - space.height) <= FIT_TOLERANCE;

    if (ocupaElHueco && restorePoint) {
      const previous = restorePoint;
      restorePoint = undefined;
      await appWindow.setPosition(new LogicalPosition(previous.x, previous.y));
      await appWindow.setSize(new LogicalSize(previous.width, previous.height));
      return;
    }
    if (!ocupaElHueco) {
      const position = (await appWindow.outerPosition()).toLogical(factor);
      restorePoint = {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
      };
    }
    await appWindow.setPosition(new LogicalPosition(space.x, space.y));
    await appWindow.setSize(new LogicalSize(space.width, space.height));
  } catch {
    // Sin ventana nativa —pruebas, navegador— el gesto no aplica.
  }
}

/**
 * ¿Este doble clic ocurrió en la parte vacía de la barra?
 *
 * Doble clic sobre un botón, un campo o un enlace es otra cosa: quien pulsa dos
 * veces seguidas en «Sincronizar» no está pidiendo que la ventana cambie de
 * tamaño.
 */
export function isEmptyChromeTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return !target.closest(
    'button, a, input, textarea, select, [role="button"], [role="tab"], [contenteditable="true"]',
  );
}
