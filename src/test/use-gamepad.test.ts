import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  advanceRepeat,
  applyDeadzone,
  buttonNameAt,
  DEFAULT_DEADZONE,
  type GamepadButtonName,
  IDLE_REPEAT,
  newPresses,
  readPressedButtons,
  resolveDirection,
  STANDARD_BUTTON_NAMES,
  stickDirection,
  useGamepad,
} from "@/hooks/use-gamepad";

function buttons(...pressedIndexes: number[]) {
  return Array.from({ length: STANDARD_BUTTON_NAMES.length }, (_unused, index) => ({
    pressed: pressedIndexes.includes(index),
  }));
}

describe("zona muerta", () => {
  it("ignora el reposo del stick y reescala el resto del recorrido", () => {
    expect(applyDeadzone(0)).toBe(0);
    expect(applyDeadzone(0.2)).toBe(0);
    expect(applyDeadzone(-0.35)).toBe(0);
    // Justo al salir de la zona muerta el valor arranca cerca de cero, no de
    // 0,35: si no, el stick daría un salto perceptible en el primer grado útil.
    expect(applyDeadzone(0.36)).toBeCloseTo(0.0154, 3);
    expect(applyDeadzone(1)).toBe(1);
    expect(applyDeadzone(-1)).toBe(-1);
  });

  it("recorta valores imposibles en lugar de propagarlos", () => {
    expect(applyDeadzone(Number.NaN)).toBe(0);
    expect(applyDeadzone(2)).toBe(1);
    // Una zona muerta absurda no puede dividir por cero.
    expect(applyDeadzone(1, 1)).toBe(1);
  });

  it("elige el eje dominante y respeta el signo del estándar", () => {
    expect(stickDirection(0, -1)).toBe("up");
    expect(stickDirection(0, 1)).toBe("down");
    expect(stickDirection(-1, 0)).toBe("left");
    expect(stickDirection(1, 0)).toBe("right");
    // En diagonal gana el eje más empujado: nunca se mueve en dos ejes a la vez.
    expect(stickDirection(0.9, -0.5)).toBe("right");
    expect(stickDirection(0.5, -0.9)).toBe("up");
    expect(stickDirection(0.1, 0.1)).toBeUndefined();
  });

  it("admite una zona muerta a medida", () => {
    expect(stickDirection(0.5, 0, DEFAULT_DEADZONE)).toBe("right");
    expect(stickDirection(0.5, 0, 0.8)).toBeUndefined();
  });
});

describe("mapeo de botones", () => {
  it("traduce los índices del mapeo estándar a nombres semánticos", () => {
    expect(buttonNameAt(0)).toBe("accept");
    expect(buttonNameAt(1)).toBe("cancel");
    expect(buttonNameAt(2)).toBe("alternate");
    expect(buttonNameAt(3)).toBe("context");
    expect(buttonNameAt(4)).toBe("leftShoulder");
    expect(buttonNameAt(5)).toBe("rightShoulder");
    expect(buttonNameAt(9)).toBe("start");
    expect(buttonNameAt(12)).toBe("dpadUp");
    expect(buttonNameAt(15)).toBe("dpadRight");
    // Un mando con más botones de los que fija el estándar no inventa nombres.
    expect(buttonNameAt(42)).toBeUndefined();
  });

  it("lee los botones pulsados de un fotograma", () => {
    expect([...readPressedButtons(buttons(0, 12))]).toEqual(["accept", "dpadUp"]);
    expect([...readPressedButtons(buttons())]).toEqual([]);
  });

  it("sólo emite el flanco de pulsación, nunca el botón mantenido", () => {
    const empty: ReadonlySet<GamepadButtonName> = new Set();
    const holding = readPressedButtons(buttons(0));
    expect(newPresses(empty, holding)).toEqual(["accept"]);
    // Segundo fotograma con la A todavía apretada: no se vuelve a lanzar nada.
    expect(newPresses(holding, holding)).toEqual([]);
    expect(newPresses(holding, empty)).toEqual([]);
  });

  it("la cruceta manda sobre el stick", () => {
    const pressed = readPressedButtons(buttons(13));
    const reading = { axes: [1, 0], buttons: buttons(13) };
    expect(resolveDirection(reading, pressed)).toBe("down");
    const idle = readPressedButtons(buttons());
    expect(resolveDirection({ axes: [1, 0], buttons: buttons() }, idle)).toBe("right");
    expect(resolveDirection({ axes: [], buttons: [] }, idle)).toBeUndefined();
  });
});

describe("repetición con retardo inicial", () => {
  const timing = { delayMs: 400, intervalMs: 120 };

  it("emite el primer paso al instante y luego espera el retardo", () => {
    const first = advanceRepeat(IDLE_REPEAT, "down", 0, timing);
    expect(first.emit).toBe(true);
    expect(first.repeat).toBe(false);

    // Mientras dura la pausa inicial, mantener el stick no mueve nada.
    const holding = advanceRepeat(first.state, "down", 399, timing);
    expect(holding.emit).toBe(false);

    const second = advanceRepeat(first.state, "down", 400, timing);
    expect(second.emit).toBe(true);
    expect(second.repeat).toBe(true);

    const waiting = advanceRepeat(second.state, "down", 519, timing);
    expect(waiting.emit).toBe(false);

    const third = advanceRepeat(second.state, "down", 520, timing);
    expect(third.emit).toBe(true);
  });

  it("no acumula pasos atrasados tras un tirón de la interfaz", () => {
    const first = advanceRepeat(IDLE_REPEAT, "up", 0, timing);
    // Un fotograma perdido de un segundo entero: se emite un paso, no nueve.
    const afterStall = advanceRepeat(first.state, "up", 2_000, timing);
    expect(afterStall.emit).toBe(true);
    // Y la cadencia se recupera desde el instante real, sin ráfaga pendiente.
    expect(advanceRepeat(afterStall.state, "up", 2_001, timing).emit).toBe(false);
    expect(advanceRepeat(afterStall.state, "up", 2_120, timing).emit).toBe(true);
  });

  it("un cambio de dirección reinicia el reloj", () => {
    const down = advanceRepeat(IDLE_REPEAT, "down", 0, timing);
    const left = advanceRepeat(down.state, "left", 50, timing);
    expect(left.emit).toBe(true);
    expect(left.repeat).toBe(false);
    expect(left.state.startedAt).toBe(50);
    // Y sigue respetando la pausa antes de repetir la dirección nueva.
    expect(advanceRepeat(left.state, "left", 300, timing).emit).toBe(false);
  });

  it("soltar el stick devuelve la repetición al reposo", () => {
    const down = advanceRepeat(IDLE_REPEAT, "down", 0, timing);
    const released = advanceRepeat(down.state, undefined, 10, timing);
    expect(released.emit).toBe(false);
    expect(released.state).toEqual(IDLE_REPEAT);
    // Volver a empujar cuenta como pulsación nueva, con paso inmediato.
    expect(advanceRepeat(released.state, "down", 20, timing).emit).toBe(true);
  });
});

// ── Bucle de sondeo ────────────────────────────────────────────────────────

interface FakeGamepad {
  id: string;
  mapping: string;
  connected: boolean;
  axes: number[];
  buttons: { pressed: boolean }[];
}

function fakeGamepad(overrides: Partial<FakeGamepad> = {}): FakeGamepad {
  return {
    id: "Mando de prueba",
    mapping: "standard",
    connected: true,
    axes: [0, 0, 0, 0],
    buttons: buttons(),
    ...overrides,
  };
}

let pending: FrameRequestCallback[] = [];
const originalRequest = window.requestAnimationFrame;
const originalCancel = window.cancelAnimationFrame;
const originalGetGamepads = Object.getOwnPropertyDescriptor(Navigator.prototype, "getGamepads");

function installFrameQueue() {
  pending = [];
  window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
    pending.push(callback);
    return pending.length;
  }) as typeof window.requestAnimationFrame;
  window.cancelAnimationFrame = vi.fn();
}

function stepFrame() {
  const queued = pending;
  pending = [];
  act(() => {
    for (const callback of queued) callback(0);
  });
}

function installGamepads(pads: (FakeGamepad | null)[]) {
  Object.defineProperty(navigator, "getGamepads", {
    configurable: true,
    writable: true,
    value: () => pads,
  });
}

function removeGamepadApi() {
  Object.defineProperty(navigator, "getGamepads", {
    configurable: true,
    writable: true,
    value: undefined,
  });
}

afterEach(() => {
  window.requestAnimationFrame = originalRequest;
  window.cancelAnimationFrame = originalCancel;
  if (originalGetGamepads) {
    Object.defineProperty(navigator, "getGamepads", originalGetGamepads);
  } else {
    Reflect.deleteProperty(navigator, "getGamepads");
  }
});

describe("bucle de sondeo", () => {
  it("degrada a inactivo donde no existe la Gamepad API", () => {
    installFrameQueue();
    removeGamepadApi();
    const onSignal = vi.fn();
    const { result } = renderHook(() => useGamepad({ onSignal }));

    expect(result.current.supported).toBe(false);
    expect(result.current.connected).toBe(false);
    // Sin API no se sondea: ni un fotograma pedido.
    expect(pending).toHaveLength(0);
    expect(onSignal).not.toHaveBeenCalled();
  });

  it("no consume fotogramas mientras no hay mando conectado", () => {
    installFrameQueue();
    installGamepads([null]);
    const { result } = renderHook(() => useGamepad({ onSignal: vi.fn() }));

    expect(result.current.supported).toBe(true);
    expect(result.current.connected).toBe(false);
    expect(pending).toHaveLength(0);
  });

  it("emite pulsaciones y direcciones del mando conectado", () => {
    installFrameQueue();
    const pad = fakeGamepad();
    installGamepads([pad]);
    const onSignal = vi.fn();
    const { result } = renderHook(() => useGamepad({ onSignal }));

    stepFrame();
    expect(result.current.connected).toBe(true);
    expect(result.current.id).toBe("Mando de prueba");
    expect(result.current.standardMapping).toBe(true);
    expect(onSignal).not.toHaveBeenCalled();

    pad.buttons = buttons(0);
    stepFrame();
    expect(onSignal).toHaveBeenCalledWith({ kind: "button", button: "accept" });

    // Mantener la A no vuelve a lanzar nada.
    onSignal.mockClear();
    stepFrame();
    expect(onSignal).not.toHaveBeenCalled();

    pad.buttons = buttons();
    pad.axes = [0, -1, 0, 0];
    stepFrame();
    expect(onSignal).toHaveBeenCalledWith({ kind: "direction", direction: "up", repeat: false });
  });

  it("señala el mando que no declara el mapeo estándar", () => {
    installFrameQueue();
    installGamepads([fakeGamepad({ mapping: "", id: "Volante raro" })]);
    const { result } = renderHook(() => useGamepad({ onSignal: vi.fn() }));

    stepFrame();
    expect(result.current.connected).toBe(true);
    expect(result.current.standardMapping).toBe(false);
  });

  it("prefiere el mando con mapeo estándar sobre cualquier otro dispositivo", () => {
    installFrameQueue();
    installGamepads([
      fakeGamepad({ mapping: "", id: "Pedales" }),
      fakeGamepad({ id: "Mando bueno" }),
    ]);
    const { result } = renderHook(() => useGamepad({ onSignal: vi.fn() }));

    stepFrame();
    expect(result.current.id).toBe("Mando bueno");
  });

  it("con `enabled` en falso no registra nada", () => {
    installFrameQueue();
    installGamepads([fakeGamepad()]);
    const { result } = renderHook(() => useGamepad({ onSignal: vi.fn(), enabled: false }));

    expect(pending).toHaveLength(0);
    expect(result.current.connected).toBe(false);
  });

  it("se desconecta limpiamente al desmontar", () => {
    installFrameQueue();
    const pad = fakeGamepad();
    installGamepads([pad]);
    const onSignal = vi.fn();
    const { unmount } = renderHook(() => useGamepad({ onSignal }));

    stepFrame();
    expect(pending).toHaveLength(1);
    unmount();
    expect(window.cancelAnimationFrame).toHaveBeenCalled();

    // El fotograma que quedaba encolado ya no puede emitir nada. La cancelación
    // falsa de esta prueba no lo desencola a propósito: lo que se comprueba es
    // que el bucle se defiende solo, no que el navegador lo haga por él.
    pad.buttons = buttons(0);
    onSignal.mockClear();
    stepFrame();
    expect(onSignal).not.toHaveBeenCalled();
  });
});
