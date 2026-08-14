import { IconBooks } from "@tabler/icons-react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBoundary } from "@/components/common/ErrorBoundary";
import { LoadingState } from "@/components/common/LoadingState";
import { Button } from "@/components/ui/button";

describe("estados de carga, vacío y recuperación", () => {
  it("anuncia una carga sin exponer el indicador decorativo", () => {
    render(<LoadingState label="Sincronizando la biblioteca de Steam" />);

    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Sincronizando la biblioteca de Steam");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status.querySelector('[aria-hidden="true"]')).toBeInTheDocument();
  });

  it("explica un estado vacío y ofrece una acción real accesible", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();

    render(
      <EmptyState
        icon={IconBooks}
        title="Tu biblioteca está lista para empezar"
        description="Importa los manifiestos locales sin añadir juegos inventados."
        action={<Button onClick={onImport}>Importar desde Steam</Button>}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Tu biblioteca está lista para empezar" }),
    ).toBeVisible();
    expect(
      screen.getByText("Importa los manifiestos locales sin añadir juegos inventados."),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Importar desde Steam" }));
    expect(onImport).toHaveBeenCalledTimes(1);
  });

  it("aísla un fallo de renderizado y permite solicitar una recuperación", async () => {
    const user = userEvent.setup();
    const onReset = vi.fn();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    function BrokenView(): never {
      throw new Error("fallo deliberado de prueba");
    }

    render(
      <ErrorBoundary onReset={onReset}>
        <BrokenView />
      </ErrorBoundary>,
    );

    expect(screen.getByRole("heading", { name: "La interfaz no pudo continuar" })).toBeVisible();
    expect(screen.getByText(/Tus datos siguen en la base local/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: /Reiniciar interfaz/ }));

    expect(onReset).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalled();
  });
});
