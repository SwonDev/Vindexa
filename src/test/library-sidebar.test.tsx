import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LibrarySidebar } from "@/features/library/LibrarySidebar";
import type { AppBootstrap } from "@/lib/types";

const bootstrap = {
  stats: { totalGames: 2, installedGames: 1, familyCatalogGames: 17, externalStoreGames: {} },
  statuses: [
    {
      id: "playing",
      name: "Jugando",
      color: "#66c0f4",
      position: 0,
      builtIn: true,
      gameCount: 1,
    },
  ],
  collections: [
    {
      id: "siempre",
      name: "A los que siempre volver",
      description: "",
      color: "#5CAAC1",
      icon: "rocket",
      kind: "manual",
      matchMode: "all",
      position: 0,
      gameCount: 46,
    },
  ],
} as AppBootstrap;

describe("barra lateral de biblioteca", () => {
  it("colapsa Estados con semántica accesible y conserva Steam Family como alcance", async () => {
    const user = userEvent.setup();
    const onScopeChange = vi.fn();
    // Los menús rápidos de estados, colecciones y tiendas escriben en la base
    // y refrescan el arranque, así que la barra lateral vive dentro del cliente
    // de consultas igual que en la aplicación.
    render(
      <QueryClientProvider client={new QueryClient()}>
        <TooltipProvider>
          <LibrarySidebar
            bootstrap={bootstrap}
            scope={{ kind: "all", label: "Todos los juegos" }}
            onScopeChange={onScopeChange}
            onCreateCollection={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    const toggle = screen.getByRole("button", { name: "ESTADOS" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: /Jugando/ })).toBeVisible();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /Jugando/ })).not.toBeInTheDocument();

    // El recuento del catálogo de Family se ve **sin** haber entrado: sale del
    // arranque. Cuando dependía del listado del propio ámbito, había que entrar
    // para ver el número que te dice que hay algo dentro.
    // Ídem que en la barra de herramientas: el contador lleva un doble oculto
    // que reserva el ancho, así que se mira el que se lee.
    expect(
      screen.getByText("17", { selector: "[data-slot='animated-number-value']" }),
    ).toBeVisible();

    await user.click(screen.getByRole("button", { name: /Steam Family/ }));
    expect(onScopeChange).toHaveBeenCalledWith({ kind: "family", label: "Steam Family" });
  });

  it("pinta el icono que la colección tiene elegido", () => {
    // Ha fallado ya una vez: la barra lateral pintaba una carpeta fija y el
    // icono elegido sólo se veía en la pantalla de colecciones, así que
    // cambiarlo parecía no hacer nada donde más se mira.
    const { container } = render(
      <QueryClientProvider client={new QueryClient()}>
        <TooltipProvider>
          <LibrarySidebar
            bootstrap={bootstrap}
            scope={{ kind: "all", label: "Todos los juegos" }}
            onScopeChange={vi.fn()}
            onCreateCollection={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    const entrada = screen.getByRole("button", { name: "A los que siempre volver" });
    expect(entrada.querySelector(".tabler-icon-rocket")).not.toBeNull();
    expect(container.querySelector(".tabler-icon-folder")).toBeNull();
  });
});
