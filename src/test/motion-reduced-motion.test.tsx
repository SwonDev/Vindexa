import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { MotionPreferencesProvider, useReducedMotion } from "@/components/motion";

function Probe() {
  const reduced = useReducedMotion();
  return <span data-testid="probe">{reduced ? "reducido" : "completo"}</span>;
}

/** Sustituye `matchMedia` por una implementación con el valor pedido. */
function mockMatchMedia(matches: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const spy = vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches,
        media: query,
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: (_: string, listener: (event: MediaQueryListEvent) => void) => {
          listeners.add(listener);
        },
        removeEventListener: (_: string, listener: (event: MediaQueryListEvent) => void) => {
          listeners.delete(listener);
        },
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
  return { spy, listeners };
}

function renderProbe(ui: ReactNode) {
  render(ui);
  return screen.getByTestId("probe");
}

describe("useReducedMotion", () => {
  it("devuelve movimiento completo cuando el sistema no pide reducirlo", () => {
    mockMatchMedia(false);
    expect(renderProbe(<Probe />)).toHaveTextContent("completo");
  });

  it("consulta exactamente la media query de reducción de movimiento", () => {
    const { spy } = mockMatchMedia(true);
    renderProbe(<Probe />);
    expect(spy).toHaveBeenCalledWith("(prefers-reduced-motion: reduce)");
  });

  it("suprime el movimiento cuando el sistema lo pide", () => {
    mockMatchMedia(true);
    expect(renderProbe(<Probe />)).toHaveTextContent("reducido");
  });

  it("se suscribe a los cambios de la preferencia y se da de baja al desmontar", () => {
    const { listeners } = mockMatchMedia(false);
    const view = render(<Probe />);
    expect(listeners.size).toBe(1);
    view.unmount();
    expect(listeners.size).toBe(0);
  });

  it("permite que un ajuste de la aplicación fuerce la supresión", () => {
    mockMatchMedia(false);
    const element = renderProbe(
      <MotionPreferencesProvider reduceMotion={true}>
        <Probe />
      </MotionPreferencesProvider>,
    );
    expect(element).toHaveTextContent("reducido");
  });

  it("permite que un ajuste de la aplicación fuerce el movimiento completo", () => {
    mockMatchMedia(true);
    const element = renderProbe(
      <MotionPreferencesProvider reduceMotion={false}>
        <Probe />
      </MotionPreferencesProvider>,
    );
    expect(element).toHaveTextContent("completo");
  });

  it("delega en el sistema con el valor automático", () => {
    mockMatchMedia(true);
    const element = renderProbe(
      <MotionPreferencesProvider reduceMotion="auto">
        <Probe />
      </MotionPreferencesProvider>,
    );
    expect(element).toHaveTextContent("reducido");
  });
});
