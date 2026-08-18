import { useEffect, useRef, useState } from "react";

/**
 * Entrada de mando de Vindexa.
 *
 * El hook no reparte el estado crudo de la Gamepad API: emite señales de alto
 * nivel —«aceptar», «cancelar», «arriba»— porque una vista no tiene por qué
 * saber que `buttons[0]` es la A de un mando de Xbox ni que el eje vertical
 * crece hacia abajo.
 *
 * Tres decisiones condicionan todo lo demás:
 *
 * - **Sondeo con `requestAnimationFrame`.** La Gamepad API no emite eventos de
 *   pulsación: obliga a leer el estado. El bucle sólo existe mientras hay un
 *   mando conectado y la vista está activa; sin mando no se gasta ni un
 *   fotograma.
 * - **Zona muerta y repetición con retardo.** Un stick en reposo devuelve
 *   valores distintos de cero, y uno mantenido daría sesenta movimientos por
 *   segundo. Se ignora todo lo que no supere la zona muerta y se repite como un
 *   teclado: primer paso inmediato, pausa, y a partir de ahí cadencia fija.
 * - **Nombres semánticos.** El mapeo estándar del W3C fija qué índice ocupa
 *   cada botón. Un mando que no lo declare se lee igual —no hay otra cosa que
 *   hacer— pero se anuncia como no estándar para que la vista avise de que los
 *   botones pueden no coincidir y recuerde que el teclado sigue funcionando.
 */

// ── Mapeo de botones ───────────────────────────────────────────────────────

/**
 * Orden del «Standard Gamepad» del W3C, que es el que declaran los mandos de
 * Xbox, PlayStation, Switch Pro y la propia Steam Deck. El índice del array es
 * el índice de `Gamepad.buttons`.
 */
export const STANDARD_BUTTON_NAMES = [
  /** 0 · A / Cruz: acción principal. */
  "accept",
  /** 1 · B / Círculo: volver o salir. */
  "cancel",
  /** 2 · X / Cuadrado: acción secundaria. */
  "alternate",
  /** 3 · Y / Triángulo: acción terciaria. */
  "context",
  "leftShoulder",
  "rightShoulder",
  "leftTrigger",
  "rightTrigger",
  /** 8 · Select / Back / Share. */
  "select",
  /** 9 · Start / Options. */
  "start",
  "leftStick",
  "rightStick",
  "dpadUp",
  "dpadDown",
  "dpadLeft",
  "dpadRight",
  /** 16 · Guía / PS / Home. Muchos sistemas la interceptan antes que la web. */
  "home",
] as const;

export type GamepadButtonName = (typeof STANDARD_BUTTON_NAMES)[number];

export type GamepadDirection = "up" | "down" | "left" | "right";

/** Señal de alto nivel: lo único que sale del hook hacia la vista. */
export type GamepadSignal =
  | { kind: "button"; button: GamepadButtonName }
  /** `repeat` distingue la primera pulsación de las repeticiones sostenidas. */
  | { kind: "direction"; direction: GamepadDirection; repeat: boolean };

/** Lectura mínima que necesita el hook; `Gamepad` la cumple estructuralmente. */
export interface GamepadButtonReading {
  pressed: boolean;
}

export interface GamepadReading {
  axes: readonly number[];
  buttons: readonly GamepadButtonReading[];
  id?: string;
  mapping?: string;
}

export function buttonNameAt(index: number): GamepadButtonName | undefined {
  return STANDARD_BUTTON_NAMES[index];
}

/** Nombres de los botones pulsados en este fotograma. */
export function readPressedButtons(
  buttons: readonly GamepadButtonReading[],
): ReadonlySet<GamepadButtonName> {
  const pressed = new Set<GamepadButtonName>();
  for (const [index, button] of buttons.entries()) {
    if (!button?.pressed) continue;
    const name = buttonNameAt(index);
    if (name) pressed.add(name);
  }
  return pressed;
}

/**
 * Botones que acaban de pulsarse. Sólo interesa el flanco: mantener A no debe
 * lanzar el juego sesenta veces por segundo.
 */
export function newPresses(
  previous: ReadonlySet<GamepadButtonName>,
  current: ReadonlySet<GamepadButtonName>,
): GamepadButtonName[] {
  return [...current].filter((name) => !previous.has(name));
}

// ── Zona muerta y dirección ────────────────────────────────────────────────

/**
 * Zona muerta por defecto. Los sticks analógicos reposan alrededor de 0,1–0,2
 * y se descentran con el uso; 0,35 deja margen sobrante sin obligar a llevar el
 * stick al tope.
 */
export const DEFAULT_DEADZONE = 0.35;

/**
 * Aplica la zona muerta reescalando el resto del recorrido. Sin reescalar, el
 * primer valor útil daría un salto de 0 a 0,35 y el stick se sentiría trabado.
 */
export function applyDeadzone(value: number, deadzone: number = DEFAULT_DEADZONE): number {
  if (!Number.isFinite(value)) return 0;
  const limit = Math.min(0.9, Math.max(0, deadzone));
  const magnitude = Math.abs(value);
  if (magnitude <= limit) return 0;
  const scaled = (magnitude - limit) / (1 - limit);
  return Math.sign(value) * Math.min(1, scaled);
}

/** Dirección del stick izquierdo, o `undefined` si está dentro de la zona muerta. */
export function stickDirection(
  x: number,
  y: number,
  deadzone: number = DEFAULT_DEADZONE,
): GamepadDirection | undefined {
  const horizontal = applyDeadzone(x, deadzone);
  const vertical = applyDeadzone(y, deadzone);
  if (horizontal === 0 && vertical === 0) return undefined;
  // En diagonal manda el eje dominante: moverse en dos ejes a la vez con un
  // gesto que la persona percibe como uno solo desordena la rejilla.
  if (Math.abs(horizontal) >= Math.abs(vertical)) return horizontal > 0 ? "right" : "left";
  // El eje vertical del estándar crece hacia abajo.
  return vertical > 0 ? "down" : "up";
}

const DPAD_DIRECTIONS: readonly [GamepadButtonName, GamepadDirection][] = [
  ["dpadUp", "up"],
  ["dpadDown", "down"],
  ["dpadLeft", "left"],
  ["dpadRight", "right"],
];

/**
 * Dirección sostenida en este fotograma. La cruceta manda sobre el stick: es
 * digital y quien la usa está pidiendo un paso exacto.
 */
export function resolveDirection(
  reading: GamepadReading,
  pressed: ReadonlySet<GamepadButtonName>,
  deadzone: number = DEFAULT_DEADZONE,
): GamepadDirection | undefined {
  for (const [button, direction] of DPAD_DIRECTIONS) {
    if (pressed.has(button)) return direction;
  }
  return stickDirection(reading.axes[0] ?? 0, reading.axes[1] ?? 0, deadzone);
}

// ── Repetición con retardo inicial ─────────────────────────────────────────

/** Pausa antes de que una dirección mantenida empiece a repetirse. */
export const REPEAT_DELAY_MS = 400;
/** Cadencia de la repetición una vez arrancada. */
export const REPEAT_INTERVAL_MS = 120;

export interface RepeatTiming {
  delayMs: number;
  intervalMs: number;
}

export const DEFAULT_REPEAT_TIMING: RepeatTiming = {
  delayMs: REPEAT_DELAY_MS,
  intervalMs: REPEAT_INTERVAL_MS,
};

export interface RepeatState {
  direction?: GamepadDirection | undefined;
  /** Instante del primer paso de la dirección actual. */
  startedAt: number;
  /** Pasos ya emitidos, incluido el primero. */
  emitted: number;
}

export const IDLE_REPEAT: RepeatState = { startedAt: 0, emitted: 0 };

export interface RepeatTick {
  state: RepeatState;
  /** Hay que emitir un movimiento en este fotograma. */
  emit: boolean;
  /** El movimiento viene de mantener la dirección, no de pulsarla. */
  repeat: boolean;
}

/**
 * Avanza la repetición un fotograma.
 *
 * Nunca devuelve más de un paso aunque el bucle se haya quedado atascado: tras
 * un tirón de la interfaz interesa recuperar la cadencia, no ejecutar de golpe
 * los diez movimientos que «tocaban». El contador sí se resincroniza para que
 * la cadencia siguiente sea la correcta.
 */
export function advanceRepeat(
  state: RepeatState,
  direction: GamepadDirection | undefined,
  now: number,
  timing: RepeatTiming = DEFAULT_REPEAT_TIMING,
): RepeatTick {
  if (!direction) return { state: IDLE_REPEAT, emit: false, repeat: false };
  if (state.direction !== direction) {
    // Primer paso inmediato: un mando tiene que responder al instante.
    return {
      state: { direction, startedAt: now, emitted: 1 },
      emit: true,
      repeat: false,
    };
  }
  const elapsed = now - state.startedAt;
  // Pasos que deberían haberse emitido ya: el primero, y uno más por cada
  // intervalo cumplido desde que terminó la pausa inicial.
  const due =
    elapsed < timing.delayMs
      ? 1
      : 2 + Math.floor((elapsed - timing.delayMs) / Math.max(1, timing.intervalMs));
  if (due <= state.emitted) return { state, emit: false, repeat: true };
  return { state: { ...state, emitted: due }, emit: true, repeat: true };
}

// ── Hook ───────────────────────────────────────────────────────────────────

export interface GamepadStatus {
  /** El entorno expone la Gamepad API. En jsdom es `false`. */
  supported: boolean;
  connected: boolean;
  /** Nombre que declara el mando; sirve para el mensaje de la interfaz. */
  id?: string | undefined;
  /** `false` con un mando que no declara el mapeo estándar del W3C. */
  standardMapping: boolean;
}

export interface UseGamepadOptions {
  /** Se llama con cada señal de alto nivel. */
  onSignal: (signal: GamepadSignal) => void;
  /** Con `false` no se registra nada y no hay bucle. */
  enabled?: boolean;
  deadzone?: number;
  repeatDelayMs?: number;
  repeatIntervalMs?: number;
}

const DISCONNECTED: GamepadStatus = {
  supported: true,
  connected: false,
  standardMapping: true,
};

const UNSUPPORTED: GamepadStatus = {
  supported: false,
  connected: false,
  standardMapping: true,
};

const NO_BUTTONS: ReadonlySet<GamepadButtonName> = new Set();

function supportsGamepads(): boolean {
  return typeof navigator !== "undefined" && typeof navigator.getGamepads === "function";
}

function timestamp(): number {
  return typeof performance !== "undefined" && typeof performance.now === "function"
    ? performance.now()
    : Date.now();
}

/**
 * Mando activo. Se prefiere uno con mapeo estándar: si hay conectado un volante
 * o un dispositivo raro junto al mando, el mando es el que debe mandar.
 */
function pickGamepad(): Gamepad | undefined {
  let fallback: Gamepad | undefined;
  let pads: (Gamepad | null)[] = [];
  try {
    pads = navigator.getGamepads();
  } catch {
    // Un contexto sin permiso para enumerar mandos equivale a no tener ninguno.
    return undefined;
  }
  for (const pad of pads) {
    if (!pad?.connected) continue;
    if (pad.mapping === "standard") return pad;
    fallback ??= pad;
  }
  return fallback;
}

function sameStatus(left: GamepadStatus, right: GamepadStatus): boolean {
  return (
    left.supported === right.supported &&
    left.connected === right.connected &&
    left.id === right.id &&
    left.standardMapping === right.standardMapping
  );
}

/**
 * Lee la Gamepad API y emite señales. Devuelve el estado de la conexión para
 * que la vista pueda decir si hay mando y si su mapeo es el estándar.
 */
export function useGamepad({
  onSignal,
  enabled = true,
  deadzone = DEFAULT_DEADZONE,
  repeatDelayMs = REPEAT_DELAY_MS,
  repeatIntervalMs = REPEAT_INTERVAL_MS,
}: UseGamepadOptions): GamepadStatus {
  const [status, setStatus] = useState<GamepadStatus>(() =>
    supportsGamepads() ? DISCONNECTED : UNSUPPORTED,
  );

  /**
   * La llamada viaja en una referencia: si entrara en las dependencias del
   * efecto, cada repintado de la vista desmontaría y remontaría el bucle y la
   * repetición perdería su reloj a mitad de un movimiento.
   */
  const signalRef = useRef(onSignal);
  signalRef.current = onSignal;

  useEffect(() => {
    if (!supportsGamepads()) {
      setStatus(UNSUPPORTED);
      return;
    }
    if (!enabled) {
      setStatus(DISCONNECTED);
      return;
    }

    let frame = 0;
    let running = false;
    /**
     * El fotograma que ya estaba encolado cuando la vista se desmonta no debe
     * emitir nada: la señal llegaría a un componente que ya no existe. Cancelar
     * el fotograma no basta como única defensa, porque el bucle se reprograma a
     * sí mismo y el orden entre limpieza y callback no está garantizado.
     */
    let disposed = false;
    let repeat: RepeatState = IDLE_REPEAT;
    let previousButtons: ReadonlySet<GamepadButtonName> = NO_BUTTONS;
    const timing: RepeatTiming = { delayMs: repeatDelayMs, intervalMs: repeatIntervalMs };

    const publish = (next: GamepadStatus) => {
      setStatus((current) => (sameStatus(current, next) ? current : next));
    };

    const tick = () => {
      frame = 0;
      if (disposed) return;
      const pad = pickGamepad();
      if (!pad) {
        // Sin mando el bucle se detiene: volverá con `gamepadconnected`.
        repeat = IDLE_REPEAT;
        previousButtons = NO_BUTTONS;
        running = false;
        publish(DISCONNECTED);
        return;
      }
      publish({
        supported: true,
        connected: true,
        id: pad.id,
        standardMapping: pad.mapping === "standard",
      });

      const pressed = readPressedButtons(pad.buttons);
      for (const button of newPresses(previousButtons, pressed)) {
        signalRef.current({ kind: "button", button });
      }
      previousButtons = pressed;

      const direction = resolveDirection(
        { axes: pad.axes, buttons: pad.buttons },
        pressed,
        deadzone,
      );
      const advanced = advanceRepeat(repeat, direction, timestamp(), timing);
      repeat = advanced.state;
      if (advanced.emit && direction) {
        signalRef.current({ kind: "direction", direction, repeat: advanced.repeat });
      }

      frame = window.requestAnimationFrame(tick);
    };

    const start = () => {
      if (running) return;
      running = true;
      frame = window.requestAnimationFrame(tick);
    };

    const onConnected = () => start();
    window.addEventListener("gamepadconnected", onConnected);
    window.addEventListener("gamepaddisconnected", onConnected);
    // Un mando ya emparejado antes de abrir la vista no dispara ningún evento.
    if (pickGamepad()) start();

    return () => {
      disposed = true;
      window.removeEventListener("gamepadconnected", onConnected);
      window.removeEventListener("gamepaddisconnected", onConnected);
      if (frame) window.cancelAnimationFrame(frame);
      running = false;
    };
  }, [deadzone, enabled, repeatDelayMs, repeatIntervalMs]);

  return status;
}
