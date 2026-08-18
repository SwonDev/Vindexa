/**
 * Guardas del «cromo de webview» de Vindexa.
 *
 * Vindexa es una aplicación de escritorio, no una página. El webview que la
 * hospeda arrastra comportamientos propios del navegador —menú contextual de
 * WebKit, arrastre nativo de imágenes, zoom con Ctrl/Cmd, selección accidental
 * de texto al arrastrar y «swipe atrás» del trackpad— que delatan la naturaleza
 * web de la ventana y rompen la ilusión de aplicación instalada.
 *
 * Este módulo suprime esos comportamientos de forma reversible y sin depender
 * de ninguna API de Tauri, de modo que funciona igual en el binario, en
 * `pnpm dev` dentro de un navegador y en jsdom durante las pruebas.
 */

/** Campos donde el menú nativo sí aporta (cortar, copiar, pegar, dictado). */
const EDITABLE_SELECTOR = [
  // Los `input` no textuales (botones, casillas, rangos) no necesitan menú nativo.
  "input:not([type='button']):not([type='checkbox']):not([type='color'])" +
    ":not([type='file']):not([type='image']):not([type='radio'])" +
    ":not([type='range']):not([type='reset']):not([type='submit'])",
  "textarea",
  "[contenteditable='']",
  "[contenteditable='true']",
  "[contenteditable='plaintext-only']",
].join(",");

/**
 * Superficies donde la selección por arrastre sigue permitida: los campos
 * editables y todo lo que se marque explícitamente con `data-selectable`
 * (descripciones, notas, rutas copiables…).
 */
const SELECTABLE_OPT_IN_SELECTOR = "[data-selectable]";

/** Elementos con arrastre nativo propio del navegador que hay que neutralizar. */
const NATIVE_DRAG_SELECTOR = "img, picture, video, a[href]";

/** Escotilla para permitir el arrastre nativo en casos concretos y explícitos. */
const ALLOW_DRAG_SELECTOR = "[data-allow-native-drag='true']";

/**
 * Teclas de zoom del webview. `Mod+0` queda fuera a propósito: los atajos de
 * Vindexa admiten dígitos (`Mod+1`…`Mod+4`) y el usuario puede reasignarlos,
 * así que bloquearlo aquí podría anular un atajo legítimo de la aplicación.
 */
const ZOOM_KEYS = new Set(["+", "-", "=", "_"]);

export interface WebviewChromeOptions {
  /** Documento sobre el que se instalan las guardas. Inyectable para pruebas. */
  document?: Document;
  /**
   * Permite abrir el menú nativo manteniendo Shift. Por omisión solo en
   * desarrollo, para poder llegar a «Inspeccionar elemento» sin recompilar.
   */
  allowNativeMenuWithShift?: boolean;
}

/** Resuelve el `Element` asociado a un objetivo de evento (nodos de texto incluidos). */
function toElement(target: EventTarget | null): Element | null {
  if (!target || typeof target !== "object") return null;
  const node = target as Node;
  if (typeof node.nodeType !== "number") return null;
  if (node.nodeType === Node.ELEMENT_NODE) return node as Element;
  return (node as ChildNode).parentElement ?? null;
}

function matchesClosest(target: EventTarget | null, selector: string): boolean {
  const element = toElement(target);
  if (!element || typeof element.closest !== "function") return false;
  return element.closest(selector) !== null;
}

/** ¿El objetivo del evento es un campo de texto editable? */
export function isEditableTarget(target: EventTarget | null): boolean {
  const element = toElement(target);
  if (!element) return false;
  // `isContentEditable` no está implementado en jsdom: solo se usa si existe.
  const editableFlag = (element as Partial<HTMLElement>).isContentEditable;
  if (editableFlag === true) return true;
  return matchesClosest(element, EDITABLE_SELECTOR);
}

/** ¿El objetivo admite selección de texto (editable u opt-in `data-selectable`)? */
export function isSelectableTarget(target: EventTarget | null): boolean {
  return isEditableTarget(target) || matchesClosest(target, SELECTABLE_OPT_IN_SELECTOR);
}

/**
 * ¿Hay una selección de texto real que incluya al objetivo? Con texto
 * seleccionado el menú nativo sigue siendo útil (copiar, buscar, dictado),
 * así que en ese caso no se bloquea.
 */
export function hasTextSelectionAt(target: EventTarget | null, doc: Document): boolean {
  const selection = typeof doc.getSelection === "function" ? doc.getSelection() : null;
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return false;
  if (selection.toString().trim().length === 0) return false;
  const element = toElement(target);
  if (!element) return false;
  if (typeof selection.containsNode === "function") {
    try {
      return selection.containsNode(element, true);
    } catch {
      // Algunas implementaciones lanzan con nodos desconectados; ante la duda,
      // se respeta el menú nativo en lugar de robarle la selección al usuario.
      return true;
    }
  }
  return true;
}

/** ¿Existe un ancestro que pueda absorber el desplazamiento horizontal pedido? */
function canScrollHorizontally(target: EventTarget | null, deltaX: number, doc: Document): boolean {
  if (deltaX === 0) return true;
  const view = doc.defaultView;
  let element = toElement(target);
  while (element) {
    const scrollWidth = element.scrollWidth ?? 0;
    const clientWidth = element.clientWidth ?? 0;
    if (scrollWidth > clientWidth + 1) {
      const overflowX = view?.getComputedStyle
        ? view.getComputedStyle(element).overflowX
        : "visible";
      if (overflowX === "auto" || overflowX === "scroll" || overflowX === "overlay") {
        const left = element.scrollLeft;
        const room = deltaX < 0 ? left : scrollWidth - clientWidth - left;
        if (room > 1) return true;
      }
    }
    element = element.parentElement;
  }
  return false;
}

function isDevEnvironment(): boolean {
  try {
    return import.meta.env.DEV === true;
  } catch {
    return false;
  }
}

/**
 * Instala las guardas y devuelve la función que las retira por completo.
 * Es segura en cualquier entorno: si no hay `document`, no hace nada.
 */
export function installWebviewChromeGuards(options: WebviewChromeOptions = {}): () => void {
  const doc = options.document ?? (typeof document === "undefined" ? undefined : document);
  if (!doc) return () => undefined;

  const allowNativeMenuWithShift = options.allowNativeMenuWithShift ?? isDevEnvironment();
  const teardown: Array<() => void> = [];

  function listen(
    type: string,
    handler: (event: Event) => void,
    listenerOptions: AddEventListenerOptions,
  ): void {
    doc?.addEventListener(type, handler, listenerOptions);
    teardown.push(() => doc?.removeEventListener(type, handler, listenerOptions));
  }

  // 1. Menú contextual nativo. Se intercepta en captura para que ningún
  //    componente intermedio pueda dejar pasar el menú de WebKit. Radix abre su
  //    propio menú en su manejador y no consulta `defaultPrevented`, así que
  //    adelantarnos no interfiere con `GameContextMenu`.
  listen(
    "contextmenu",
    (event) => {
      const mouseEvent = event as MouseEvent;
      if (allowNativeMenuWithShift && mouseEvent.shiftKey) return;
      if (isEditableTarget(mouseEvent.target)) return;
      if (hasTextSelectionAt(mouseEvent.target, doc)) return;
      event.preventDefault();
    },
    { capture: true },
  );

  // 2. Selección de texto por arrastre en superficies no textuales. Se resuelve
  //    con `selectstart` en lugar de `user-select: none` global porque la CSP de
  //    producción (`style-src 'self'`) prohíbe inyectar hojas de estilo en
  //    tiempo de ejecución, y porque así los campos editables conservan intacto
  //    su comportamiento nativo.
  listen(
    "selectstart",
    (event) => {
      if (isSelectableTarget(event.target)) return;
      event.preventDefault();
    },
    { capture: true },
  );

  // 3. Arrastre nativo de imágenes y enlaces (el fantasma translúcido del
  //    navegador). No afecta a dnd-kit, que trabaja con eventos de puntero.
  listen(
    "dragstart",
    (event) => {
      if (matchesClosest(event.target, ALLOW_DRAG_SELECTOR)) return;
      if (matchesClosest(event.target, NATIVE_DRAG_SELECTOR)) event.preventDefault();
    },
    { capture: true },
  );

  // 4. Zoom con rueda + Ctrl/Cmd y «swipe atrás» del trackpad. Ambos llegan como
  //    `wheel`, así que comparten manejador. `passive: false` es imprescindible
  //    para poder cancelarlos.
  listen(
    "wheel",
    (event) => {
      const wheelEvent = event as WheelEvent;
      if (wheelEvent.ctrlKey || wheelEvent.metaKey) {
        event.preventDefault();
        return;
      }
      const horizontal = Math.abs(wheelEvent.deltaX) > Math.abs(wheelEvent.deltaY);
      if (horizontal && !canScrollHorizontally(wheelEvent.target, wheelEvent.deltaX, doc)) {
        // Sobredesplazamiento horizontal sin destino: es el gesto de navegación
        // atrás, que sacaría al usuario de la aplicación.
        event.preventDefault();
      }
    },
    { capture: true, passive: false },
  );

  // 5. Zoom por teclado del webview.
  listen(
    "keydown",
    (event) => {
      const keyEvent = event as KeyboardEvent;
      if (!(keyEvent.metaKey || keyEvent.ctrlKey) || keyEvent.altKey) return;
      if (!ZOOM_KEYS.has(keyEvent.key)) return;
      event.preventDefault();
    },
    { capture: true },
  );

  // 6. Refuerzo declarativo del «swipe atrás»: se aplica como propiedad inline
  //    del elemento raíz, permitido por `style-src-attr 'unsafe-inline'`.
  const root = doc.documentElement;
  const previousOverscroll = root?.style.getPropertyValue("overscroll-behavior-x") ?? "";
  root?.style.setProperty("overscroll-behavior-x", "none");
  teardown.push(() => {
    if (!root) return;
    if (previousOverscroll) root.style.setProperty("overscroll-behavior-x", previousOverscroll);
    else root.style.removeProperty("overscroll-behavior-x");
  });

  return () => {
    while (teardown.length > 0) {
      const dispose = teardown.pop();
      dispose?.();
    }
  };
}
