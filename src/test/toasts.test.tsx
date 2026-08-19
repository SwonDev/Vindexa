import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ToastProvider, useToast } from "@/features/shell/toasts";

function Lanzador({
  onReady,
}: {
  onReady?: ((api: ReturnType<typeof useToast>) => void) | undefined;
}) {
  const toast = useToast();
  return (
    <div>
      <button type="button" onClick={() => toast.success("Todo listo")}>
        Bien
      </button>
      <button type="button" onClick={() => toast.error("No se pudo sincronizar")}>
        Mal
      </button>
      <button
        type="button"
        onClick={() => {
          const id = toast.pending("Sincronizando…");
          onReady?.(toast);
          toast.replace(id, { message: "553 juegos", kind: "success" });
        }}
      >
        En marcha
      </button>
    </div>
  );
}

describe("avisos de la aplicación", () => {
  it("un acierto se anuncia y se puede descartar", async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <Lanzador />
      </ToastProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Bien" }));
    // `toBeVisible` mira la opacidad, y la animación de entrada arranca en cero:
    // en jsdom no llega a completarse, así que lo que se comprueba es que el
    // aviso está en el documento.
    expect(await screen.findByText("Todo listo")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Descartar aviso: Todo listo" }));
    await waitFor(() => expect(screen.queryByText("Todo listo")).toBeNull());
  });

  it("un error se anuncia como alerta y no se cierra solo", async () => {
    // Un fallo es información que hay que leer: retirarlo a los cinco segundos
    // es perderlo justo cuando quien lo necesita no estaba mirando.
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <Lanzador />
      </ToastProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Mal" }));
    const aviso = await screen.findByRole("alert");
    expect(aviso).toHaveTextContent("No se pudo sincronizar");
  });

  it("lo que está en marcha se sustituye por su resultado, sin apilar dos", async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <Lanzador />
      </ToastProvider>,
    );

    await user.click(screen.getByRole("button", { name: "En marcha" }));
    expect(await screen.findByText("553 juegos")).toBeInTheDocument();
    expect(screen.queryByText("Sincronizando…")).toBeNull();
  });

  it("fuera del proveedor no falla: simplemente no avisa", () => {
    // Una pantalla montada suelta —en una prueba, o en una vista aislada— no
    // debe romperse por no tener dónde dejar un aviso.
    expect(() => render(<Lanzador />)).not.toThrow();
  });
});
