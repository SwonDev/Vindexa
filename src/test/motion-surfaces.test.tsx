import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  DragFeedbackSurface,
  MotionPreferencesProvider,
  PressableSurface,
  ShimmerSkeleton,
} from "@/components/motion";

function dropSurface(): HTMLElement {
  const element = document.querySelector<HTMLElement>('[data-slot="drag-feedback-surface"]');
  if (!element) throw new Error("no se encontró la zona de destino");
  return element;
}

describe("PressableSurface", () => {
  it("renderiza un botón real que responde al clic", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(<PressableSurface onClick={onClick}>Sincronizar</PressableSurface>);

    const button = screen.getByRole("button", { name: "Sincronizar" });
    expect(button).toHaveAttribute("type", "button");
    await user.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("recorta la elevación y la escala a los máximos del sistema de diseño", () => {
    render(
      <PressableSurface liftPx={40} hoverScale={1.4} pressScale={0.2}>
        Acción
      </PressableSurface>,
    );

    const button = screen.getByRole("button");
    expect(button.style.getPropertyValue("--vx-press-lift")).toBe("-4px");
    expect(button.style.getPropertyValue("--vx-press-hover-scale")).toBe("1.02");
    expect(button.style.getPropertyValue("--vx-press-active-scale")).toBe("0.96");
  });

  it("mantiene los valores contenidos por defecto", () => {
    render(<PressableSurface>Acción</PressableSurface>);
    const button = screen.getByRole("button");
    expect(button.style.getPropertyValue("--vx-press-lift")).toBe("-1px");
    expect(button.style.getPropertyValue("--vx-press-hover-scale")).toBe("1.01");
  });

  it("apaga el realce con movimiento reducido", () => {
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <PressableSurface>Acción</PressableSurface>
      </MotionPreferencesProvider>,
    );
    expect(screen.getByRole("button")).toHaveAttribute("data-motion", "off");
  });

  it("apaga el realce cuando se pide explícitamente, sin perder el botón", () => {
    render(<PressableSurface motionDisabled>Acción</PressableSurface>);
    expect(screen.getByRole("button")).toHaveAttribute("data-motion", "off");
  });

  it("aplica el comportamiento a un elemento existente con asChild", () => {
    render(
      <PressableSurface asChild>
        <a href="https://store.steampowered.com">Abrir la tienda</a>
      </PressableSurface>,
    );

    const link = screen.getByRole("link", { name: "Abrir la tienda" });
    expect(link).toHaveClass("vx-pressable");
    expect(link).not.toHaveAttribute("type");
  });
});

describe("ShimmerSkeleton", () => {
  it("es decorativo por defecto y queda fuera del árbol de accesibilidad", () => {
    render(<ShimmerSkeleton height={14} />);
    const group = document.querySelector('[data-slot="shimmer-skeleton"]');
    expect(group).toHaveAttribute("aria-hidden", "true");
    expect(group?.querySelectorAll('[data-slot="shimmer-skeleton-block"]')).toHaveLength(1);
  });

  it("se anuncia como estado de carga cuando recibe una etiqueta", () => {
    render(<ShimmerSkeleton label="Cargando la biblioteca" count={3} />);
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("aria-busy", "true");
    expect(status).toHaveTextContent("Cargando la biblioteca");
    expect(status.querySelectorAll('[data-slot="shimmer-skeleton-block"]')).toHaveLength(3);
  });

  it("reserva la geometría real del contenido", () => {
    render(<ShimmerSkeleton aspectRatio="2 / 3" width={142} />);
    const block = document.querySelector<HTMLElement>('[data-slot="shimmer-skeleton-block"]');
    expect(block?.style.aspectRatio).toBe("2 / 3");
    expect(block?.style.width).toBe("142px");
    // Con proporción no se fija alto: la caja la determina el ancho real.
    expect(block?.style.height).toBe("");
  });

  it("recorta el radio a la geometría técnica del sistema", () => {
    render(<ShimmerSkeleton radiusPx={18} />);
    const block = document.querySelector<HTMLElement>('[data-slot="shimmer-skeleton-block"]');
    expect(block?.style.borderRadius).toBe("3px");
  });

  it("apaga el barrido con movimiento reducido", () => {
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <ShimmerSkeleton />
      </MotionPreferencesProvider>,
    );
    expect(document.querySelector('[data-slot="shimmer-skeleton"]')).toHaveAttribute(
      "data-shimmer",
      "false",
    );
  });

  it("permite dejarlo estático sin movimiento reducido", () => {
    render(<ShimmerSkeleton shimmer={false} />);
    expect(document.querySelector('[data-slot="shimmer-skeleton"]')).toHaveAttribute(
      "data-shimmer",
      "false",
    );
  });
});

describe("DragFeedbackSurface", () => {
  it("expone cada estado de destino en el DOM", () => {
    const view = render(<DragFeedbackSurface state="idle">Pendientes</DragFeedbackSurface>);
    expect(dropSurface()).toHaveAttribute("data-drop-state", "idle");

    for (const state of ["active", "over", "rejected"] as const) {
      view.rerender(<DragFeedbackSurface state={state}>Pendientes</DragFeedbackSurface>);
      expect(dropSurface()).toHaveAttribute("data-drop-state", state);
    }
  });

  it("muestra el contador solo con multiselección y destino activo", () => {
    const view = render(
      <DragFeedbackSurface state="idle" count={4}>
        Pendientes
      </DragFeedbackSurface>,
    );
    expect(dropSurface().querySelector(".vx-drop-surface__count")).toBeNull();

    view.rerender(
      <DragFeedbackSurface state="over" count={4}>
        Pendientes
      </DragFeedbackSurface>,
    );
    expect(dropSurface().querySelector(".vx-drop-surface__count")).toHaveTextContent("4");

    view.rerender(
      <DragFeedbackSurface state="over" count={1}>
        Pendientes
      </DragFeedbackSurface>,
    );
    expect(dropSurface().querySelector(".vx-drop-surface__count")).toBeNull();
  });

  it("muestra la pista de destino solo durante el arrastre", () => {
    const view = render(
      <DragFeedbackSurface state="idle" hint="Mover a Pendientes">
        Pendientes
      </DragFeedbackSurface>,
    );
    expect(screen.queryByText("Mover a Pendientes")).toBeNull();

    view.rerender(
      <DragFeedbackSurface state="active" hint="Mover a Pendientes">
        Pendientes
      </DragFeedbackSurface>,
    );
    expect(screen.getByText("Mover a Pendientes")).toHaveAttribute("aria-hidden", "true");
  });

  it("apaga la escala con movimiento reducido y conserva el estado", () => {
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <DragFeedbackSurface state="over">Pendientes</DragFeedbackSurface>
      </MotionPreferencesProvider>,
    );
    expect(dropSurface()).toHaveAttribute("data-motion", "off");
    expect(dropSurface()).toHaveAttribute("data-drop-state", "over");
  });

  it("realza un elemento existente sin añadir nodos con asChild", () => {
    render(
      <DragFeedbackSurface state="over" asChild>
        <button type="button" className="sidebar-item">
          Pendientes
        </button>
      </DragFeedbackSurface>,
    );

    const target = screen.getByRole("button", { name: "Pendientes" });
    expect(target).toHaveClass("sidebar-item", "vx-drop-surface");
    expect(target).toHaveAttribute("data-drop-state", "over");
  });
});
