import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

describe("contratos de interacción accesible", () => {
  it("permite buscar mediante una etiqueta visible y conserva el texto Unicode", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();

    render(
      <div>
        <label htmlFor="library-search">Buscar en la biblioteca</label>
        <Input id="library-search" onChange={onChange} />
      </div>,
    );

    const search = screen.getByRole("textbox", {
      name: "Buscar en la biblioteca",
    });
    await user.type(search, "acción y exploración");

    expect(search).toHaveValue("acción y exploración");
    expect(onChange).toHaveBeenCalled();
  });

  it("activa acciones principales con teclado y respeta el estado deshabilitado", async () => {
    const user = userEvent.setup();
    const onSync = vi.fn();
    const onUnavailable = vi.fn();

    render(
      <div>
        <Button onClick={onSync}>Sincronizar biblioteca</Button>
        <Button disabled onClick={onUnavailable}>
          Instalar juego
        </Button>
      </div>,
    );

    const syncButton = screen.getByRole("button", {
      name: "Sincronizar biblioteca",
    });
    syncButton.focus();
    await user.keyboard("{Enter}");
    await user.click(screen.getByRole("button", { name: "Instalar juego" }));

    expect(onSync).toHaveBeenCalledTimes(1);
    expect(onUnavailable).not.toHaveBeenCalled();
  });

  it("expone selección binaria con nombre y cambio de estado comprensible", async () => {
    const user = userEvent.setup();

    function Preferences() {
      const [periodicSync, setPeriodicSync] = useState(false);
      const [tracking, setTracking] = useState(false);

      return (
        <div>
          <label htmlFor="periodic-sync">Sincronización periódica</label>
          <Switch id="periodic-sync" checked={periodicSync} onCheckedChange={setPeriodicSync} />
          <label htmlFor="tracking">Seguir actualizaciones</label>
          <Checkbox
            id="tracking"
            checked={tracking}
            onCheckedChange={(value) => setTracking(value === true)}
          />
        </div>
      );
    }

    render(<Preferences />);

    const syncSwitch = screen.getByRole("switch", {
      name: "Sincronización periódica",
    });
    const trackingCheckbox = screen.getByRole("checkbox", {
      name: "Seguir actualizaciones",
    });
    await user.click(syncSwitch);
    await user.click(trackingCheckbox);

    expect(syncSwitch).toBeChecked();
    expect(trackingCheckbox).toBeChecked();
  });

  it("abre un diálogo etiquetado, mueve el foco y puede cerrarlo con Escape", async () => {
    const user = userEvent.setup();

    render(
      <Dialog>
        <DialogTrigger asChild>
          <Button>Abrir ajustes de Steam</Button>
        </DialogTrigger>
        <DialogContent>
          <DialogTitle>Conexión con Steam</DialogTitle>
          <DialogDescription>Configura tu cuenta sin almacenar la contraseña.</DialogDescription>
          <Button>Conectar cuenta</Button>
        </DialogContent>
      </Dialog>,
    );

    await user.click(screen.getByRole("button", { name: "Abrir ajustes de Steam" }));
    const dialog = screen.getByRole("dialog", { name: "Conexión con Steam" });

    expect(dialog).toHaveAccessibleDescription("Configura tu cuenta sin almacenar la contraseña.");
    expect(screen.getByRole("button", { name: "Conectar cuenta" })).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Conexión con Steam" })).not.toBeInTheDocument();
  });

  it("permite recorrer secciones con la semántica de pestañas", async () => {
    const user = userEvent.setup();

    render(
      <Tabs defaultValue="library">
        <TabsList aria-label="Secciones principales">
          <TabsTrigger value="library">Biblioteca</TabsTrigger>
          <TabsTrigger value="planner">Planificador</TabsTrigger>
        </TabsList>
        <TabsContent value="library">Todos tus juegos</TabsContent>
        <TabsContent value="planner">Tu próxima partida</TabsContent>
      </Tabs>,
    );

    const planner = screen.getByRole("tab", { name: "Planificador" });
    await user.click(planner);

    expect(planner).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Tu próxima partida")).toBeVisible();
    expect(screen.queryByText("Todos tus juegos")).not.toBeInTheDocument();
  });
});
