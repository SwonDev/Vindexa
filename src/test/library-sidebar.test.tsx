import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LibrarySidebar } from "@/features/library/LibrarySidebar";
import type { AppBootstrap } from "@/lib/types";

const bootstrap = {
  stats: {
    totalGames: 2,
    installedGames: 1,
    familyCatalogGames: 17,
    // Con cero juegos el atajo de la tienda no se pinta —y entonces su menú no
    // se puede probar—, así que aquí hay una tienda con algo dentro.
    externalStoreGames: { epic: 553 },
  },
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

/**
 * El clic derecho en la barra lateral, sobre los componentes de verdad.
 *
 * # El fallo que estas pruebas habrían visto
 *
 * Los menús estaban escritos y montados y no abría ninguno: `SidebarItem` y
 * `CollectionSidebarItem` no reenviaban al elemento del DOM ni el `ref` ni las
 * props que no reconocían, y `ContextMenuTrigger asChild` clona a su hijo
 * pasándole justo eso. El `onContextMenu` se quedaba por el camino.
 *
 * Las pruebas que ya había montaban cada menú con `<button>Texto</button>` como
 * hijo —un hijo que sí reenvía todo—, así que pasaban en verde mientras la
 * aplicación no abría nada. Éstas montan la barra lateral entera, que es lo que
 * usa la persona.
 */
describe("el clic derecho en la barra lateral abre sus menús", () => {
  function montar() {
    return render(
      <QueryClientProvider client={new QueryClient()}>
        <TooltipProvider>
          <LibrarySidebar
            bootstrap={bootstrap}
            scope={{ kind: "all", label: "Todos los juegos" }}
            onScopeChange={vi.fn()}
            onCreateCollection={vi.fn()}
            onEditCollection={vi.fn()}
            onDeleteCollection={vi.fn()}
            onEditStatuses={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );
  }

  it("una tienda ofrece abrirla en el navegador integrado", async () => {
    const user = userEvent.setup();
    montar();

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: /Epic Games/ }),
    });

    const menu = await screen.findByRole("menu");
    expect(within(menu).getByText(/Abrir en el navegador integrado/)).toBeVisible();
  });

  it("un estado ofrece sus acciones", async () => {
    const user = userEvent.setup();
    montar();

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: /Jugando/ }),
    });

    expect(await screen.findByRole("menu")).toBeVisible();
  });

  it("y una colección también", async () => {
    const user = userEvent.setup();
    montar();

    // Hay dos botones con ese nombre: el atajo y el asa de reordenar. El
    // menú cuelga del primero, que es el que se pulsa.
    const [atajo] = screen.getAllByRole("button", { name: /A los que siempre volver/ });
    await user.pointer({ keys: "[MouseRight]", target: atajo as HTMLElement });

    expect(await screen.findByRole("menu")).toBeVisible();
  });
});

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
