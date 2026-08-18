import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { CopyableValue, IconMorph, MotionPreferencesProvider } from "@/components/motion";

function root(): HTMLElement {
  const element = document.querySelector<HTMLElement>('[data-slot="copyable-value"]');
  if (!element) throw new Error("no se encontró el valor copiable");
  return element;
}

describe("CopyableValue", () => {
  it("muestra el valor y describe la acción sin ambigüedad", () => {
    render(<CopyableValue value="440" copy={vi.fn()} />);
    const button = screen.getByRole("button", { name: "Copiar 440" });
    expect(button).toHaveTextContent("440");
    expect(button).toHaveAttribute("data-status", "idle");
  });

  it("admite un texto visible distinto del valor copiado", () => {
    render(<CopyableValue value="/Users/juegos/steamapps" display="steamapps" copy={vi.fn()} />);
    expect(screen.getByRole("button")).toHaveTextContent("steamapps");
    expect(screen.getByRole("button")).toHaveAccessibleName("Copiar /Users/juegos/steamapps");
  });

  it("copia el valor y confirma el resultado", async () => {
    const user = userEvent.setup();
    const copy = vi.fn().mockResolvedValue(undefined);
    const onCopied = vi.fn();
    render(<CopyableValue value="440" copy={copy} onCopied={onCopied} />);

    await user.click(screen.getByRole("button"));

    expect(copy).toHaveBeenCalledExactlyOnceWith("440");
    await waitFor(() => {
      expect(root()).toHaveAttribute("data-status", "copied");
    });
    expect(onCopied).toHaveBeenCalledExactlyOnceWith("440");
    expect(screen.getByRole("status")).toHaveTextContent("Copiado");
  });

  it("anuncia el fallo en vez de fingir que copió", async () => {
    const user = userEvent.setup();
    const onCopyError = vi.fn();
    render(
      <CopyableValue
        value="440"
        copy={() => Promise.reject(new Error("clipboard_unavailable"))}
        onCopyError={onCopyError}
      />,
    );

    await user.click(screen.getByRole("button"));

    await waitFor(() => {
      expect(root()).toHaveAttribute("data-status", "error");
    });
    expect(screen.getByRole("status")).toHaveTextContent("No se pudo copiar");
    expect(onCopyError).toHaveBeenCalledTimes(1);
  });

  it("vuelve al estado de reposo pasada la confirmación", async () => {
    const user = userEvent.setup();
    render(<CopyableValue value="440" copy={vi.fn()} confirmMs={0} />);

    await user.click(screen.getByRole("button"));
    await waitFor(() => {
      expect(root()).toHaveAttribute("data-status", "copied");
    });
    await waitFor(
      () => {
        expect(root()).toHaveAttribute("data-status", "idle");
      },
      { timeout: 2000 },
    );
  });

  it("respeta un manejador de clic propio que cancela la copia", async () => {
    const user = userEvent.setup();
    const copy = vi.fn();
    render(<CopyableValue value="440" copy={copy} onClick={(event) => event.preventDefault()} />);

    await user.click(screen.getByRole("button"));
    expect(copy).not.toHaveBeenCalled();
  });

  it("usa el portapapeles de la plataforma cuando no le pasan otro", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(<CopyableValue value="440" />);
    await user.click(screen.getByRole("button"));

    expect(writeText).toHaveBeenCalledExactlyOnceWith("440");
  });

  it("apaga el movimiento del icono con la preferencia reducida", () => {
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <CopyableValue value="440" copy={vi.fn()} />
      </MotionPreferencesProvider>,
    );
    expect(root()).toHaveAttribute("data-motion", "off");
  });
});

function MorphHarness() {
  const [confirmed, setConfirmed] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setConfirmed((value) => !value)}>
        Conmutar
      </button>
      <IconMorph
        confirmed={confirmed}
        icon={<span data-testid="reposo">copiar</span>}
        confirmIcon={<span data-testid="confirmado">hecho</span>}
        sizePx={16}
      />
    </>
  );
}

describe("IconMorph", () => {
  it("es decorativo y reserva una caja fija", () => {
    render(
      <IconMorph
        confirmed={false}
        icon={<span>a</span>}
        confirmIcon={<span>b</span>}
        sizePx={18}
      />,
    );

    const morph = document.querySelector<HTMLElement>('[data-slot="icon-morph"]');
    expect(morph).toHaveAttribute("aria-hidden", "true");
    expect(morph?.style.width).toBe("18px");
    expect(morph?.style.height).toBe("18px");
  });

  it("cruza al icono de confirmación y vuelve", async () => {
    const user = userEvent.setup();
    render(<MorphHarness />);

    expect(screen.getByTestId("reposo")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Conmutar" }));
    expect(screen.getByTestId("confirmado")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByTestId("reposo")).toBeNull();
    });

    await user.click(screen.getByRole("button", { name: "Conmutar" }));
    await waitFor(() => {
      expect(screen.queryByTestId("confirmado")).toBeNull();
    });
    expect(screen.getByTestId("reposo")).toBeInTheDocument();
  });

  it("refleja el estado de confirmación en el DOM", () => {
    const view = render(
      <IconMorph confirmed={false} icon={<span>a</span>} confirmIcon={<span>b</span>} />,
    );
    expect(document.querySelector('[data-slot="icon-morph"]')).toHaveAttribute(
      "data-confirmed",
      "false",
    );

    view.rerender(<IconMorph confirmed icon={<span>a</span>} confirmIcon={<span>b</span>} />);
    expect(document.querySelector('[data-slot="icon-morph"]')).toHaveAttribute(
      "data-confirmed",
      "true",
    );
  });
});
