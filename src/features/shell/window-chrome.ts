/**
 * Ajuste de la ventana al abrirse.
 *
 * # Por qué el arrastre y el doble clic ya no viven aquí
 *
 * Vivieron, y no funcionaban. Resolver el gesto a mano obliga a acertar con el
 * orden exacto en que cada sistema entrega los eventos —en macOS, pedir el
 * arrastre al pulsar hace que el segundo clic no llegue nunca— y a mantener esa
 * pieza para siempre. El propio Tauri ya trae ese trabajo hecho y probado en su
 * zona de arrastre, así que la barra lo declara con `data-tauri-drag-region` y
 * este módulo se queda sólo con lo que Tauri no cubre: abrir ocupando el hueco
 * real de la pantalla.
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
  if (typeof window === "undefined" || typeof window.screen === "undefined") return;
  const appWindow = await currentWindow();
  if (!appWindow) return;
  const { availWidth, availHeight } = window.screen;
  if (!availWidth || !availHeight) return;
  // `availLeft` y `availTop` no están en todos los motores; su ausencia
  // significa que el área útil empieza en el origen.
  const left = (window.screen as Screen & { availLeft?: number }).availLeft ?? 0;
  const top = (window.screen as Screen & { availTop?: number }).availTop ?? 0;
  try {
    const { LogicalPosition, LogicalSize } = await import("@tauri-apps/api/dpi");
    await appWindow.setPosition(new LogicalPosition(left, top));
    await appWindow.setSize(new LogicalSize(availWidth, availHeight));
  } catch {
    // Sin ventana nativa —pruebas, navegador— no hay nada que ajustar.
  }
}
