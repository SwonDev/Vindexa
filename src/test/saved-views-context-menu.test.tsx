import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { SavedLibraryView } from "@/features/library/library-views";
import { SavedViewsBar } from "@/features/library/SavedViewsBar";

/**
 * El clic derecho sobre una vista guardada.
 *
 * # Por qué esta prueba y no una lectura del código
 *
 * El disparador del menú convive aquí con un `Tooltip`, y `Tooltip` no pinta
 * nada en el DOM: colgarle `asChild` encima deja un menú que no se abre nunca,
 * sin que falle el tipado ni la compilación. Sólo se ve montándolo y pulsando.
 */

const vista: SavedLibraryView = {
  id: "v1",
  name: "Cortos y pendientes",
  accent: "cyan",
  pinned: false,
  query: {},
  createdAt: "2026-01-01T10:00:00Z",
} as SavedLibraryView;

function renderBarra(overrides: Partial<Parameters<typeof SavedViewsBar>[0]> = {}) {
  const props = {
    creating: false,
    onCreatingChange: vi.fn(),
    views: [vista],
    activeIds: [] as string[],
    onToggle: vi.fn(),
    onSave: vi.fn(),
    onUpdate: vi.fn(),
    onDelete: vi.fn(),
    onTogglePinned: vi.fn(),
    currentQuery: {},
    conflicts: [],
    ...overrides,
  } as Parameters<typeof SavedViewsBar>[0];
  render(
    <TooltipProvider>
      <SavedViewsBar {...props} />
    </TooltipProvider>,
  );
  return props;
}

describe("acciones rápidas de una vista guardada", () => {
  it("el clic derecho abre el menú con las mismas opciones que el botón «⋯»", async () => {
    const user = userEvent.setup();
    renderBarra();

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: "Cortos y pendientes" }),
    });

    const menu = await screen.findByRole("menu", {
      name: /Acciones rápidas de Cortos y pendientes/,
    });
    expect(within(menu).getByRole("menuitem", { name: "Aplicar" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Anclar al principio" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Eliminar vista" })).toBeVisible();
  });

  it("anclar desde el menú hace lo mismo que anclar desde el botón", async () => {
    const user = userEvent.setup();
    const props = renderBarra();

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: "Cortos y pendientes" }),
    });
    await user.click(await screen.findByRole("menuitem", { name: "Anclar al principio" }));

    expect(props.onTogglePinned).toHaveBeenCalledWith(expect.objectContaining({ id: "v1" }));
  });

  it("actualizar está apagado mientras la vista no esté aplicada", async () => {
    // Actualizar «con lo que veo» sin tener la vista puesta guardaría otra cosa
    // distinta de la que se está mirando.
    const user = userEvent.setup();
    renderBarra();

    await user.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: "Cortos y pendientes" }),
    });
    const menu = await screen.findByRole("menu", { name: /Acciones rápidas/ });
    expect(
      within(menu).getByRole("menuitem", { name: /Actualizar con lo que veo/ }),
    ).toHaveAttribute("data-disabled");
  });
});
