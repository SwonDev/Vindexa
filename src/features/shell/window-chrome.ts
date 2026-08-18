/**
 * Comportamiento de la barra superior como barra de título nativa.
 *
 * La ventana se declara con `titleBarStyle: "Overlay"` y `hiddenTitle: true`,
 * así que la cabecera de Vindexa *es* la barra de título del sistema. Aquí se
 * resuelven los dos gestos que el sistema operativo esperaría de ella:
 * arrastrar la ventana desde una zona vacía y maximizar o restaurar con doble
 * clic.
 *
 * Se hace desde JavaScript en lugar de con `data-tauri-drag-region` para tener
 * control exacto sobre qué cuenta como «zona vacía» y para que el doble clic no
 * se dispare dos veces (una por el gestor nativo y otra por el de la interfaz).
 */

/** Elementos que capturan el gesto: sobre ellos la barra no arrastra ni maximiza. */
const INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  "kbd",
  '[role="button"]',
  '[role="tab"]',
  '[role="menuitem"]',
  '[contenteditable="true"]',
  "[data-no-window-drag]",
].join(", ");

/** `true` cuando el gesto ocurrió sobre una parte inerte de la barra de título. */
export function isWindowDragArea(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest(INTERACTIVE_SELECTOR)) return false;
  const selection = target.ownerDocument?.defaultView?.getSelection();
  // Si hay texto seleccionado el gesto pertenece a la selección, no a la ventana.
  return !selection || selection.isCollapsed;
}

async function currentWindow() {
  try {
    const module = await import("@tauri-apps/api/window");
    return module.getCurrentWindow();
  } catch {
    // Fuera del contenedor de escritorio (pruebas, `pnpm dev` en navegador) no
    // hay ventana nativa que manipular y el gesto simplemente no aplica.
    return undefined;
  }
}

/** Alterna entre maximizado y restaurado, como haría la barra de título nativa. */
export async function toggleWindowMaximize(): Promise<void> {
  const appWindow = await currentWindow();
  if (!appWindow) return;
  try {
    await appWindow.toggleMaximize();
  } catch {
    // Una ventana no redimensionable no puede maximizarse: no es un error.
  }
}

/** Cede el gesto al gestor de ventanas para mover la ventana con el puntero. */
export async function startWindowDrag(): Promise<void> {
  const appWindow = await currentWindow();
  if (!appWindow) return;
  try {
    await appWindow.startDragging();
  } catch {
    // Ídem: sin ventana nativa el arrastre no aplica.
  }
}

/**
 * Manejador único de `mousedown` para la barra de título.
 *
 * `event.detail === 2` identifica el segundo clic de un doble clic. Resolver
 * ambos gestos en el mismo evento es imprescindible: en cuanto se llama a
 * `startDragging()` el sistema se apodera del puntero y el `dblclick` posterior
 * ya nunca llegaría al documento.
 */
export function handleTitleBarPointerDown(event: {
  button: number;
  detail: number;
  target: EventTarget | null;
  preventDefault: () => void;
}): "maximize" | "drag" | "ignore" {
  if (event.button !== 0) return "ignore";
  if (!isWindowDragArea(event.target)) return "ignore";
  event.preventDefault();
  if (event.detail >= 2) {
    void toggleWindowMaximize();
    return "maximize";
  }
  void startWindowDrag();
  return "drag";
}
