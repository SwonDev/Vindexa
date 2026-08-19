import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CollectionContextMenu,
  QUICK_COLLECTION_COLORS,
  StoreContextMenu,
} from "@/features/library/SidebarContextMenus";
import { api } from "@/lib/tauri";
import type { AppBootstrap } from "@/lib/types";

vi.mock("@/lib/tauri", async (original) => {
  const actual = await original<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      setCollectionAppearance: vi.fn(async () => undefined),
      openStoreBrowser: vi.fn(async () => undefined),
    },
  };
});

const coleccion = {
  id: "col-1",
  name: "A los que siempre volver",
  description: "",
  color: "#5CAAC1",
  icon: "folder",
  kind: "manual",
  matchMode: "all",
  position: 0,
  gameCount: 46,
} as unknown as NonNullable<AppBootstrap["collections"]>[number];

function envolver(children: React.ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(<QueryClientProvider client={client}>{children}</QueryClientProvider>);
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("acciones rápidas de la barra lateral", () => {
  it("cambia el color de una colección sin tocar su icono", async () => {
    const user = userEvent.setup();
    envolver(
      <CollectionContextMenu collection={coleccion} onEdit={vi.fn()} onDelete={vi.fn()}>
        <button type="button">A los que siempre volver</button>
      </CollectionContextMenu>,
    );

    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button") });
    // El submenú se abre con el teclado, que es lo que jsdom reproduce de forma
    // fiable: el puntero de Radix depende de medidas que aquí no existen.
    await screen.findByText("Color");
    await user.keyboard("{ArrowDown}{ArrowRight}");
    const verde = QUICK_COLLECTION_COLORS[1];
    await user.click(await screen.findByRole("menuitem", { name: verde.label }));

    // El icono viaja con el cambio para que la orden nunca lo deje vacío, y el
    // resto de la colección —nombre, descripción, reglas— ni se envía.
    await waitFor(() =>
      expect(api.setCollectionAppearance).toHaveBeenCalledWith("col-1", verde.value, "folder"),
    );
  });

  it("cambia el icono conservando el color", async () => {
    const user = userEvent.setup();
    envolver(
      <CollectionContextMenu collection={coleccion} onEdit={vi.fn()} onDelete={vi.fn()}>
        <button type="button">A los que siempre volver</button>
      </CollectionContextMenu>,
    );

    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button") });
    await screen.findByText("Icono");
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowRight}");
    await user.click(await screen.findByRole("menuitem", { name: /Trofeo/ }));

    await waitFor(() =>
      expect(api.setCollectionAppearance).toHaveBeenCalledWith("col-1", "#5CAAC1", "trophy"),
    );
  });

  it("deja editar y borrar en manos de quien las pide, no del menú", async () => {
    const user = userEvent.setup();
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    envolver(
      <CollectionContextMenu collection={coleccion} onEdit={onEdit} onDelete={onDelete}>
        <button type="button">A los que siempre volver</button>
      </CollectionContextMenu>,
    );

    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button") });
    await user.click(await screen.findByRole("menuitem", { name: /Editar colección/ }));
    expect(onEdit).toHaveBeenCalledOnce();
    // Borrar no borra desde aquí: quien lo recibe pide confirmación primero.
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("abre una tienda en el navegador integrado con su propia sesión", async () => {
    const user = userEvent.setup();
    envolver(
      <StoreContextMenu storeId="gog" storeLabel="GOG">
        <button type="button">GOG</button>
      </StoreContextMenu>,
    );

    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button") });
    await user.click(await screen.findByRole("menuitem", { name: /navegador integrado/ }));

    await waitFor(() => expect(api.openStoreBrowser).toHaveBeenCalledWith("gog"));
  });
});
