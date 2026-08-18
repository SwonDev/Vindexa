import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  MotionPreferencesProvider,
  SegmentedControl,
  type SegmentedControlOption,
} from "@/components/motion";

type View = "cuadricula" | "lista" | "compacta";

const OPTIONS: readonly SegmentedControlOption<View>[] = [
  { value: "cuadricula", label: "Cuadrícula" },
  { value: "lista", label: "Lista" },
  { value: "compacta", label: "Compacta" },
];

function ControlledHarness({
  initial = "cuadricula",
  onChange,
  options = OPTIONS,
}: {
  initial?: View;
  onChange?: (value: View) => void;
  options?: readonly SegmentedControlOption<View>[];
}) {
  const [value, setValue] = useState<View>(initial);
  return (
    <SegmentedControl
      options={options}
      value={value}
      onValueChange={(next) => {
        setValue(next);
        onChange?.(next);
      }}
      label="Vista de la biblioteca"
    />
  );
}

function indicator(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-slot="segmented-control-indicator"]');
}

/** Etiqueta que envuelve al radio con ese nombre accesible. */
function optionFor(name: string): HTMLElement {
  const option = screen
    .getByRole("radio", { name })
    .closest<HTMLElement>('[data-slot="segmented-control-option"]');
  if (!option) throw new Error(`no se encontró la opción ${name}`);
  return option;
}

describe("SegmentedControl", () => {
  it("expone un grupo de radios con nombre accesible", () => {
    render(<ControlledHarness />);
    expect(screen.getByRole("radiogroup", { name: "Vista de la biblioteca" })).toBeVisible();
    expect(screen.getAllByRole("radio")).toHaveLength(3);
  });

  it("marca la opción activa y solo esa", () => {
    render(<ControlledHarness />);
    expect(screen.getByRole("radio", { name: "Cuadrícula" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Lista" })).not.toBeChecked();
  });

  it("cambia de opción al pulsar y avisa una sola vez", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ControlledHarness onChange={onChange} />);

    await user.click(screen.getByRole("radio", { name: "Lista" }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith("lista");
    expect(screen.getByRole("radio", { name: "Lista" })).toBeChecked();
  });

  it("usa radios nativos agrupados, no atributos ARIA imitados", () => {
    render(<ControlledHarness initial="lista" />);
    const radios = screen.getAllByRole("radio");
    for (const radio of radios) {
      expect(radio.tagName).toBe("INPUT");
      expect(radio).toHaveAttribute("type", "radio");
      expect(radio).not.toHaveAttribute("aria-checked");
    }
    // Un único `name` para todo el grupo: el navegador se encarga del orden de
    // tabulación sin que el componente falsifique `tabindex`.
    const names = new Set(radios.map((radio) => radio.getAttribute("name")));
    expect(names.size).toBe(1);
  });

  it("recorre las opciones con las flechas y da la vuelta al final", async () => {
    const user = userEvent.setup();
    render(<ControlledHarness />);

    screen.getByRole("radio", { name: "Cuadrícula" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: "Lista" })).toBeChecked();

    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(screen.getByRole("radio", { name: "Cuadrícula" })).toBeChecked();

    await user.keyboard("{ArrowLeft}");
    expect(screen.getByRole("radio", { name: "Compacta" })).toBeChecked();
  });

  it("salta al primero y al último con Inicio y Fin", async () => {
    const user = userEvent.setup();
    render(<ControlledHarness initial="lista" />);

    screen.getByRole("radio", { name: "Lista" }).focus();
    await user.keyboard("{End}");
    expect(screen.getByRole("radio", { name: "Compacta" })).toBeChecked();

    await user.keyboard("{Home}");
    expect(screen.getByRole("radio", { name: "Cuadrícula" })).toBeChecked();
  });

  it("se salta las opciones desactivadas al navegar", async () => {
    const user = userEvent.setup();
    render(
      <ControlledHarness
        options={[
          { value: "cuadricula", label: "Cuadrícula" },
          { value: "lista", label: "Lista", disabled: true },
          { value: "compacta", label: "Compacta" },
        ]}
      />,
    );

    screen.getByRole("radio", { name: "Cuadrícula" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: "Compacta" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Lista" })).toBeDisabled();
  });

  it("dibuja el indicador dentro de la opción seleccionada y solo ahí", async () => {
    const user = userEvent.setup();
    render(<ControlledHarness />);

    expect(document.querySelectorAll('[data-slot="segmented-control-indicator"]')).toHaveLength(1);
    expect(optionFor("Cuadrícula")).toContainElement(indicator());

    await user.click(screen.getByRole("radio", { name: "Compacta" }));
    expect(optionFor("Compacta")).toContainElement(indicator());
  });

  it("sigue mostrando el indicador con movimiento reducido, sin animarlo", () => {
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <ControlledHarness />
      </MotionPreferencesProvider>,
    );
    expect(indicator()).toBeInTheDocument();
  });

  it("acepta iconos y textos de ayuda sin ensuciar el nombre accesible", () => {
    render(
      <ControlledHarness
        options={[
          { value: "cuadricula", label: "Cuadrícula", hint: "Portadas 2:3", icon: <svg /> },
          { value: "lista", label: "Lista" },
          { value: "compacta", label: "Compacta" },
        ]}
      />,
    );

    expect(screen.getByRole("radio", { name: "Cuadrícula" })).toHaveAttribute(
      "title",
      "Portadas 2:3",
    );
    expect(optionFor("Cuadrícula").querySelector(".vx-segmented__icon")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });
});
