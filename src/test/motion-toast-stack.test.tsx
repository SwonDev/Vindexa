import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { MotionPreferencesProvider, type ToastItem, ToastStack } from "@/components/motion";

const INFO: ToastItem = { id: "a", message: "Biblioteca sincronizada" };

function toasts(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-slot="toast"]'));
}

describe("ToastStack", () => {
  it("expone una región con nombre accesible y una lista", () => {
    render(<ToastStack toasts={[INFO]} onDismiss={vi.fn()} label="Avisos de sincronización" />);
    const list = screen.getByRole("list", { name: "Avisos de sincronización" });
    expect(list).toHaveAttribute("data-position", "bottom-right");
    expect(toasts()).toHaveLength(1);
  });

  it("anuncia los avisos normales como estado y los errores como alerta", () => {
    render(
      <ToastStack
        toasts={[
          INFO,
          { id: "b", message: "No se pudo leer el manifiesto", kind: "error", detail: "EACCES" },
        ]}
        onDismiss={vi.fn()}
        max={5}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("Biblioteca sincronizada");
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("No se pudo leer el manifiesto");
    expect(alert).toHaveTextContent("EACCES");
  });

  it("conserva los avisos más recientes cuando se supera el máximo", () => {
    render(
      <ToastStack
        toasts={[
          { id: "1", message: "Uno" },
          { id: "2", message: "Dos" },
          { id: "3", message: "Tres" },
          { id: "4", message: "Cuatro" },
        ]}
        onDismiss={vi.fn()}
        max={2}
      />,
    );

    expect(toasts()).toHaveLength(2);
    expect(screen.getByText("Tres")).toBeInTheDocument();
    expect(screen.getByText("Cuatro")).toBeInTheDocument();
    expect(screen.queryByText("Uno")).toBeNull();
  });

  it("permite descartar un aviso a mano con una etiqueta inequívoca", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    render(<ToastStack toasts={[INFO]} onDismiss={onDismiss} />);

    await user.click(
      screen.getByRole("button", { name: "Descartar aviso: Biblioteca sincronizada" }),
    );
    expect(onDismiss).toHaveBeenCalledExactlyOnceWith("a");
  });

  it("ofrece una acción única y la ejecuta sin cerrar el aviso por su cuenta", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    const onDismiss = vi.fn();
    render(
      <ToastStack
        toasts={[{ ...INFO, action: { label: "Deshacer", onClick } }]}
        onDismiss={onDismiss}
        autoDismissMs={0}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Deshacer" }));
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("se retira solo tras el plazo indicado", async () => {
    const onDismiss = vi.fn();
    render(<ToastStack toasts={[INFO]} onDismiss={onDismiss} autoDismissMs={30} />);

    await waitFor(() => {
      expect(onDismiss).toHaveBeenCalledExactlyOnceWith("a");
    });
  });

  it("nunca retira un error por su cuenta: hay que leerlo", async () => {
    const onDismiss = vi.fn();
    render(
      <ToastStack
        toasts={[{ id: "e", message: "Fallo de red", kind: "error" }]}
        onDismiss={onDismiss}
        autoDismissMs={20}
      />,
    );

    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("respeta un plazo propio por aviso", async () => {
    const onDismiss = vi.fn();
    render(
      <ToastStack
        toasts={[{ ...INFO, autoDismissMs: 25 }]}
        onDismiss={onDismiss}
        autoDismissMs={0}
      />,
    );

    await waitFor(() => {
      expect(onDismiss).toHaveBeenCalledExactlyOnceWith("a");
    });
  });

  it("no programa ningún cierre con el plazo desactivado", async () => {
    const onDismiss = vi.fn();
    render(<ToastStack toasts={[INFO]} onDismiss={onDismiss} autoDismissMs={0} />);

    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("distingue cada tipo con un atributo comprobable", () => {
    render(
      <ToastStack
        toasts={[
          { id: "i", message: "Info", kind: "info" },
          { id: "s", message: "Hecho", kind: "success" },
          { id: "w", message: "Ojo", kind: "warning" },
        ]}
        onDismiss={vi.fn()}
        max={5}
        autoDismissMs={0}
      />,
    );

    expect(toasts().map((node) => node.dataset.kind)).toEqual(["info", "success", "warning"]);
  });

  it("sigue mostrando los avisos con movimiento reducido", () => {
    render(
      <MotionPreferencesProvider reduceMotion={true}>
        <ToastStack toasts={[INFO]} onDismiss={vi.fn()} autoDismissMs={0} />
      </MotionPreferencesProvider>,
    );
    expect(screen.getByText("Biblioteca sincronizada")).toBeInTheDocument();
  });

  it("coloca la pila donde se le indica", () => {
    render(
      <ToastStack toasts={[INFO]} onDismiss={vi.fn()} position="top-left" autoDismissMs={0} />,
    );
    expect(screen.getByRole("list")).toHaveAttribute("data-position", "top-left");
  });
});
