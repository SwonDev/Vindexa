import type { Transition } from "motion/react";

/**
 * Tokens de movimiento de Vindexa.
 *
 * Reglas duras que estos valores materializan:
 * - Duraciones entre 120 ms y 260 ms. Solo el desplegable medido llega a 300 ms.
 * - Curvas: las mismas `--ease-out` / `--ease-in-out` declaradas en `src/index.css`.
 * - Muelles sobreamortiguados: nunca rebotan (damping > amortiguación crítica).
 * - Solo `transform` y `opacity` en cualquier animación continua.
 */

/** `cubic-bezier(0.23, 1, 0.32, 1)` — idéntica a `--ease-out`. */
export const EASE_OUT = [0.23, 1, 0.32, 1] as const;

/** `cubic-bezier(0.77, 0, 0.175, 1)` — idéntica a `--ease-in-out`. */
export const EASE_IN_OUT = [0.77, 0, 0.175, 1] as const;

/** Duraciones en segundos, que es la unidad que espera `motion`. */
export const DURATION = {
  /** 120 ms — confirmaciones y cambios de icono. */
  instant: 0.12,
  /** 160 ms — realces de puntero y estados de arrastre. */
  fast: 0.16,
  /** 200 ms — entradas y salidas normales. */
  base: 0.2,
  /** 260 ms — recorridos largos: indicador de segmentos, avisos apilados. */
  slow: 0.26,
  /** 300 ms — techo reservado para desplegables de altura medida. */
  disclosure: 0.3,
} as const;

export type DurationToken = keyof typeof DURATION;

/** Milisegundos, para las variables CSS y los `setTimeout`. */
export const DURATION_MS = {
  instant: 120,
  fast: 160,
  base: 200,
  slow: 260,
  disclosure: 300,
} as const;

/**
 * Muelle corto para desplazamientos pequeños (indicador de segmentos).
 * Amortiguación crítica = 2·√(stiffness·mass) = 2·√(520·0.6) ≈ 35.3.
 * Con `damping: 42` queda sobreamortiguado: llega y para, sin rebote.
 */
export const SPRING_SNAP: Transition = {
  type: "spring",
  stiffness: 520,
  damping: 42,
  mass: 0.6,
  restDelta: 0.4,
};

/**
 * Muelle para reordenaciones de pila (avisos que suben al cerrarse uno).
 * Crítica = 2·√(420·0.7) ≈ 34.3; con `damping: 40` tampoco rebota.
 */
export const SPRING_STACK: Transition = {
  type: "spring",
  stiffness: 420,
  damping: 40,
  mass: 0.7,
  restDelta: 0.4,
};

export const TRANSITION_FAST: Transition = { duration: DURATION.fast, ease: EASE_OUT };
export const TRANSITION_BASE: Transition = { duration: DURATION.base, ease: EASE_OUT };
export const TRANSITION_SLOW: Transition = { duration: DURATION.slow, ease: EASE_OUT };
export const TRANSITION_DISCLOSURE: Transition = {
  duration: DURATION.disclosure,
  ease: EASE_IN_OUT,
};

/** Transición sin movimiento: la que se usa cuando el sistema pide reducirlo. */
export const TRANSITION_NONE: Transition = { duration: 0 };

/**
 * Desplazamiento vertical por defecto de las apariciones. Deliberadamente
 * diminuto: en una lista densa cualquier valor mayor se lee como un salto.
 */
export const REVEAL_DISTANCE_PX = 4;

/** Retardo entre elementos de una aparición escalonada. */
export const STAGGER_STEP_MS = 24;

/**
 * Techo del retardo acumulado. Sin él, la fila 40 de una lista tardaría casi
 * un segundo en aparecer y la interfaz dejaría de sentirse instalada.
 */
export const STAGGER_MAX_MS = 160;

/** Devuelve la transición pedida, o ninguna si hay que suprimir el movimiento. */
export function withReducedMotion(transition: Transition, reduced: boolean): Transition {
  return reduced ? TRANSITION_NONE : transition;
}
