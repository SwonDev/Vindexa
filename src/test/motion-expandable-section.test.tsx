import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ExpandableSection, MotionPreferencesProvider } from "@/components/motion";

function trigger(): HTMLElement {
  return screen.getByRole("button", { name: /Sesiones/ });
}

describe("ExpandableSection", () => {
  it("arranca cerrada y anuncia su estado", () => {
    render(
      <ExpandableSection title="Sesiones">
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Registro de partidas")).toBeNull();
  });

  it("abre y cierra por sí sola cuando no está controlada", async () => {
    const user = userEvent.setup();
    render(
      <ExpandableSection title="Sesiones">
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    await user.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => {
      expect(screen.getByText("Registro de partidas")).toBeVisible();
    });

    await user.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    await waitFor(() => {
      expect(screen.queryByText("Registro de partidas")).toBeNull();
    });
  });

  it("respeta el estado inicial abierto", () => {
    render(
      <ExpandableSection title="Sesiones" defaultOpen>
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
  });

  it("cede el control cuando le pasan `open`", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    render(
      <ExpandableSection title="Sesiones" open={false} onOpenChange={onOpenChange}>
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    await user.click(trigger());
    expect(onOpenChange).toHaveBeenCalledExactlyOnceWith(true);
    // Sin que el padre cambie `open`, la sección no se abre por su cuenta.
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
  });

  it("enlaza la cabecera con el panel solo mientras el panel existe", async () => {
    const user = userEvent.setup();
    render(
      <ExpandableSection title="Sesiones">
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    expect(trigger()).not.toHaveAttribute("aria-controls");

    await user.click(trigger());
    const controls = trigger().getAttribute("aria-controls");
    expect(controls).toBeTruthy();
    expect(document.getElementById(controls as string)).toBeInTheDocument();
  });

  it("mantiene el contenido montado e inerte cuando se pide conservarlo", async () => {
    const user = userEvent.setup();
    render(
      <ExpandableSection title="Sesiones" keepMounted>
        <input aria-label="Notas de la sesión" />
      </ExpandableSection>,
    );

    const input = screen.getByLabelText("Notas de la sesión");
    const panel = document.querySelector(".vx-expandable__panel");
    expect(input).toBeInTheDocument();
    expect(panel).toHaveAttribute("aria-hidden", "true");
    expect(panel).toHaveAttribute("inert");

    await user.click(trigger());
    expect(panel).toHaveAttribute("aria-hidden", "false");
    expect(panel).not.toHaveAttribute("inert");
  });

  it("informa de la altura medida para poder revalidar un virtualizador", async () => {
    const user = userEvent.setup();
    const onHeightChange = vi.fn();
    render(
      <ExpandableSection title="Sesiones" onHeightChange={onHeightChange}>
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    expect(onHeightChange).toHaveBeenLastCalledWith(0);
    await user.click(trigger());
    await waitFor(() => {
      expect(onHeightChange).toHaveBeenCalledTimes(2);
    });
    expect(typeof onHeightChange.mock.calls.at(-1)?.[0]).toBe("number");
  });

  it("no se abre estando desactivada", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    render(
      <ExpandableSection title="Sesiones" disabled onOpenChange={onOpenChange}>
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    expect(trigger()).toBeDisabled();
    await user.click(trigger());
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("coloca el contenido auxiliar de la cabecera fuera del disparador", () => {
    render(
      <ExpandableSection title="Sesiones" headerExtra={<span>12</span>}>
        <p>Registro de partidas</p>
      </ExpandableSection>,
    );

    const extra = document.querySelector(".vx-expandable__extra");
    expect(extra).toHaveTextContent("12");
    expect(trigger()).not.toContainElement(extra as HTMLElement);
  });

  it("sigue abriendo y cerrando con movimiento reducido", async () => {
    const user = userEvent.setup();
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <ExpandableSection title="Sesiones">
          <p>Registro de partidas</p>
        </ExpandableSection>
      </MotionPreferencesProvider>,
    );

    await user.click(trigger());
    await waitFor(() => {
      expect(screen.getByText("Registro de partidas")).toBeVisible();
    });
    await user.click(trigger());
    await waitFor(() => {
      expect(screen.queryByText("Registro de partidas")).toBeNull();
    });
  });
});
