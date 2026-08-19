import { DndContext, type DragEndEvent } from "@dnd-kit/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LibrarySidebar } from "@/features/library/LibrarySidebar";
import type { AppBootstrap } from "@/lib/types";

const bootstrap = {
  stats: { totalGames: 4, installedGames: 2 },
  statuses: [],
  collections: [
    {
      id: "manual",
      name: "Cooperativos",
      description: "",
      color: "#66c0f4",
      icon: "folder",
      kind: "manual",
      matchMode: "all",
      position: 0,
      gameCount: 2,
    },
    {
      id: "smart",
      name: "Casi terminados",
      description: "",
      color: "#a4d007",
      icon: "sparkles",
      kind: "smart",
      matchMode: "all",
      position: 1,
      gameCount: 2,
    },
  ],
} as AppBootstrap;

function renderSidebar(onDragEnd = vi.fn<(event: DragEndEvent) => void>(), draggingGames = false) {
  // La barra lateral incluye los menús de acciones rápidas, que hablan con el
  // backend a través del cliente de consultas.
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <DndContext onDragEnd={onDragEnd}>
        <TooltipProvider>
          <LibrarySidebar
            bootstrap={bootstrap}
            scope={{ kind: "all", label: "Todos los juegos" }}
            draggingGames={draggingGames}
            collectionReorderEnabled
            onScopeChange={vi.fn()}
            onCreateCollection={vi.fn()}
          />
        </TooltipProvider>
      </DndContext>
    </QueryClientProvider>,
  );
  return onDragEnd;
}

describe("destinos y orden accesible de colecciones", () => {
  it("explica por qué una colección inteligente no admite juegos", () => {
    renderSidebar(undefined, true);

    expect(screen.getByRole("button", { name: /^Casi terminados/ })).toHaveAccessibleDescription(
      "Colección inteligente: no admite juegos soltados; edita sus reglas.",
    );
  });

  it("ofrece el mismo reordenado con Espacio y flechas", async () => {
    const rect = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function () {
        const index = Array.from(document.querySelectorAll(".sidebar-collection")).indexOf(this);
        return new DOMRect(0, Math.max(0, index) * 40, 220, 32);
      });
    const user = userEvent.setup();
    const onDragEnd = renderSidebar();
    const handle = screen.getByRole("button", { name: "Reordenar colección Cooperativos" });
    handle.focus();

    await user.keyboard("[Space][ArrowDown][Space]");

    expect(onDragEnd).toHaveBeenCalled();
    const event = onDragEnd.mock.calls[0]?.[0];
    expect(String(event?.active.id)).toBe("collection-order:manual");
    expect(String(event?.over?.id)).toBe("collection-order:smart");
    rect.mockRestore();
  });
});
