import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MotionPreferencesProvider, RevealOnScroll, StaggerList } from "@/components/motion";

type ObserverCallback = (entries: IntersectionObserverEntry[]) => void;

const observers: { callback: ObserverCallback; targets: Element[]; disconnected: boolean }[] = [];

class FakeIntersectionObserver {
  private readonly record: (typeof observers)[number];

  constructor(callback: ObserverCallback) {
    this.record = { callback, targets: [], disconnected: false };
    observers.push(this.record);
  }

  observe(target: Element) {
    this.record.targets.push(target);
  }

  unobserve() {}

  disconnect() {
    this.record.disconnected = true;
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
}

function installObserver() {
  observers.length = 0;
  vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
}

function intersect(index: number, isIntersecting: boolean) {
  const record = observers[index];
  if (!record) throw new Error(`no hay observador con índice ${index}`);
  act(() => {
    record.callback([{ isIntersecting } as IntersectionObserverEntry]);
  });
}

function revealNodes(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-slot="reveal-on-scroll"]'));
}

afterEach(() => {
  vi.unstubAllGlobals();
  observers.length = 0;
});

describe("RevealOnScroll", () => {
  it("aparece al entrar en el viewport y deja de observar si es una sola vez", () => {
    installObserver();
    render(
      <RevealOnScroll>
        <p>Fila de biblioteca</p>
      </RevealOnScroll>,
    );

    const node = revealNodes()[0];
    expect(node).toHaveAttribute("data-revealed", "false");

    intersect(0, true);
    expect(node).toHaveAttribute("data-revealed", "true");
    expect(observers[0]?.disconnected).toBe(true);
  });

  it("vuelve a ocultarse al salir del viewport cuando no es una sola vez", () => {
    installObserver();
    render(
      <RevealOnScroll once={false}>
        <p>Fila</p>
      </RevealOnScroll>,
    );

    intersect(0, true);
    expect(revealNodes()[0]).toHaveAttribute("data-revealed", "true");
    intersect(0, false);
    expect(revealNodes()[0]).toHaveAttribute("data-revealed", "false");
    expect(observers[0]?.disconnected).toBe(false);
  });

  it("avisa una sola vez cuando el elemento entra", () => {
    installObserver();
    const onReveal = vi.fn();
    render(
      <RevealOnScroll onReveal={onReveal}>
        <p>Fila</p>
      </RevealOnScroll>,
    );

    intersect(0, true);
    expect(onReveal).toHaveBeenCalledTimes(1);
  });

  it("se pinta visible sin IntersectionObserver, en vez de quedarse en blanco", () => {
    // jsdom no implementa IntersectionObserver: es el mismo camino que un
    // entorno empotrado sin la API.
    render(
      <RevealOnScroll>
        <p>Fila</p>
      </RevealOnScroll>,
    );
    expect(revealNodes()[0]).toHaveAttribute("data-revealed", "true");
  });

  it("se pinta visible y no llega a observar nada con movimiento reducido", () => {
    installObserver();
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <RevealOnScroll>
          <p>Fila</p>
        </RevealOnScroll>
      </MotionPreferencesProvider>,
    );

    expect(revealNodes()[0]).toHaveAttribute("data-revealed", "true");
    expect(observers).toHaveLength(0);
  });

  it("se pinta visible cuando se desactiva explícitamente", () => {
    installObserver();
    render(
      <RevealOnScroll disabled>
        <p>Fila</p>
      </RevealOnScroll>,
    );
    expect(revealNodes()[0]).toHaveAttribute("data-revealed", "true");
    expect(observers).toHaveLength(0);
  });

  it("publica retardo y distancia como variables CSS, sin tocar la caja", () => {
    installObserver();
    render(
      <RevealOnScroll delayMs={48} distancePx={6} durationMs={220}>
        <p>Fila</p>
      </RevealOnScroll>,
    );

    const node = revealNodes()[0];
    expect(node?.style.getPropertyValue("--vx-reveal-delay")).toBe("48ms");
    expect(node?.style.getPropertyValue("--vx-reveal-distance")).toBe("6px");
    expect(node?.style.getPropertyValue("--vx-reveal-duration")).toBe("220ms");
    // Nada de alto, margen ni relleno: la medición de la fila no cambia.
    expect(node?.style.height).toBe("");
    expect(node?.style.margin).toBe("");
  });

  it("no publica ninguna variable en línea cuando usa los valores por defecto", () => {
    installObserver();
    render(
      <RevealOnScroll>
        <p>Fila</p>
      </RevealOnScroll>,
    );
    expect(revealNodes()[0]?.getAttribute("style")).toBeNull();
  });

  it("no añade un nodo al DOM con asChild y conserva las clases del hijo", () => {
    installObserver();
    render(
      <RevealOnScroll asChild delayMs={12}>
        <li className="sidebar-item">Colección</li>
      </RevealOnScroll>,
    );

    const item = screen.getByRole("listitem");
    expect(item.tagName).toBe("LI");
    expect(item).toHaveClass("sidebar-item", "vx-reveal");
    expect(item).toHaveAttribute("data-revealed", "false");
    intersect(0, true);
    expect(item).toHaveAttribute("data-revealed", "true");
  });
});

describe("StaggerList", () => {
  it("escalona los retardos y los limita al techo configurado", () => {
    installObserver();
    render(
      <StaggerList stepMs={20} maxDelayMs={60}>
        <p>Uno</p>
        <p>Dos</p>
        <p>Tres</p>
        <p>Cuatro</p>
        <p>Cinco</p>
      </StaggerList>,
    );

    // El primer elemento no publica variable: sin retardo no hay nada que
    // declarar, y en una lista larga cada atributo de más es peso muerto.
    const delays = revealNodes().map((node) => node.style.getPropertyValue("--vx-reveal-delay"));
    expect(delays).toEqual(["", "20ms", "40ms", "60ms", "60ms"]);
  });

  it("continúa la cuenta desde un índice inicial", () => {
    installObserver();
    render(
      <StaggerList stepMs={20} maxDelayMs={200} startIndex={2}>
        <p>Uno</p>
        <p>Dos</p>
      </StaggerList>,
    );

    const delays = revealNodes().map((node) => node.style.getPropertyValue("--vx-reveal-delay"));
    expect(delays).toEqual(["40ms", "60ms"]);
  });

  it("puede ser la propia lista sin insertar un contenedor intermedio", () => {
    installObserver();
    render(
      <StaggerList as="ul" className="sidebar-list" itemAsChild>
        <li>Colección A</li>
        <li>Colección B</li>
      </StaggerList>,
    );

    const list = screen.getByRole("list");
    expect(list.tagName).toBe("UL");
    expect(list).toHaveClass("sidebar-list");
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    for (const item of screen.getAllByRole("listitem")) {
      expect(item).toHaveClass("vx-reveal");
    }
  });

  it("desactiva la aparición de todos los hijos de una vez", () => {
    installObserver();
    render(
      <StaggerList disabled>
        <p>Uno</p>
        <p>Dos</p>
      </StaggerList>,
    );

    for (const node of revealNodes()) {
      expect(node).toHaveAttribute("data-revealed", "true");
    }
    expect(observers).toHaveLength(0);
  });
});
