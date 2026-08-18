import { render, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AnimatedNumber, MotionPreferencesProvider } from "@/components/motion";
import { formatPlaytime } from "@/lib/format";

const es = new Intl.NumberFormat("es-ES", { maximumFractionDigits: 0 });

function valueSlot(): HTMLElement {
  const element = document.querySelector<HTMLElement>('[data-slot="animated-number-value"]');
  if (!element) throw new Error("no se encontró la cifra visible");
  return element;
}

function reserveSlot(): HTMLElement {
  const element = document.querySelector<HTMLElement>(".vx-animated-number__reserve");
  if (!element) throw new Error("no se encontró la reserva de ancho");
  return element;
}

describe("AnimatedNumber", () => {
  it("pinta la cifra formateada en español", () => {
    render(<AnimatedNumber value={1500} />);
    expect(valueSlot()).toHaveTextContent(es.format(1500));
  });

  it("reserva el ancho del valor final desde el primer fotograma", () => {
    const view = render(<AnimatedNumber value={9} />);
    expect(reserveSlot()).toHaveTextContent("9");

    view.rerender(<AnimatedNumber value={123456} />);
    // La reserva salta ya al destino: la caja no crece mientras cuenta.
    expect(reserveSlot()).toHaveTextContent(es.format(123456));
    expect(reserveSlot()).toHaveAttribute("aria-hidden", "true");
  });

  it("expone el valor exacto en el DOM para poder verificarlo", () => {
    render(<AnimatedNumber value={42} />);
    const root = document.querySelector('[data-slot="animated-number"]');
    expect(root).toHaveAttribute("data-value", "42");
  });

  it("interpola hasta el destino y termina exactamente en él", async () => {
    const view = render(<AnimatedNumber value={0} />);
    view.rerender(<AnimatedNumber value={860} />);

    await waitFor(() => {
      expect(valueSlot()).toHaveTextContent(es.format(860));
    });
    expect(document.querySelector('[data-slot="animated-number"]')).toHaveAttribute(
      "data-settled",
      "true",
    );
  });

  it("salta directamente al destino con movimiento reducido", () => {
    const view = render(
      <MotionPreferencesProvider reduceMotion={true}>
        <AnimatedNumber value={0} />
      </MotionPreferencesProvider>,
    );
    view.rerender(
      <MotionPreferencesProvider reduceMotion={true}>
        <AnimatedNumber value={7200} />
      </MotionPreferencesProvider>,
    );
    expect(valueSlot()).toHaveTextContent(es.format(7200));
  });

  it("salta directamente al destino cuando se desactiva la interpolación", () => {
    const view = render(<AnimatedNumber value={0} disabled />);
    view.rerender(<AnimatedNumber value={310} disabled />);
    expect(valueSlot()).toHaveTextContent(es.format(310));
  });

  it("acepta el formateador de tiempo de juego del repositorio", () => {
    render(<AnimatedNumber value={135} format={formatPlaytime} disabled />);
    expect(valueSlot()).toHaveTextContent("2 h 15 min");
  });

  it("acepta atributos accesibles y clases del contexto", () => {
    render(<AnimatedNumber value={3} className="detail-metric" title="Juegos instalados" />);
    const root = document.querySelector('[data-slot="animated-number"]');
    expect(root).toHaveClass("vx-animated-number", "detail-metric");
    expect(root).toHaveAttribute("title", "Juegos instalados");
  });
});
