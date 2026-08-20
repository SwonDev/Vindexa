import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { type ExtraFilters, LibraryToolbar } from "@/features/library/LibraryToolbar";
import type { LibraryFilterOptions } from "@/features/library/library-filters";
import type { GameSort, LibraryView } from "@/lib/types";

function ToolbarHarness({ note }: { note?: { text: string; detail: string } } = {}) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<GameSort>("manual");
  const [view, setView] = useState<LibraryView>("grid");
  const [filters, setFilters] = useState<ExtraFilters>({});

  return (
    <TooltipProvider>
      <LibraryToolbar
        title="Todos los juegos"
        total={2_450}
        {...(note ? { note } : {})}
        query={query}
        onQueryChange={setQuery}
        sort={sort}
        onSortChange={setSort}
        view={view}
        onViewChange={setView}
        filters={filters}
        onFiltersChange={setFilters}
        statuses={[
          { id: "playing", name: "Jugando ahora" },
          { id: "backlog", name: "Backlog" },
        ]}
        collections={[{ id: "coop", name: "Cooperativos" }]}
        filterOptions={
          {
            genres: ["Acción", "Estrategia"],
            categories: ["Cooperativo en línea"],
            tags: [{ id: "relajante", name: "Relajante" }],
            totalGames: 2_450,
            metadataGames: 2_000,
            achievementGames: 180,
            steamDeckGames: 0,
            drmGames: 0,
          } satisfies LibraryFilterOptions
        }
      />
    </TooltipProvider>
  );
}

describe("barra de herramientas de biblioteca", () => {
  it("busca, limpia la consulta y mantiene el contador localizado", async () => {
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    // El contador cuenta hacia su valor, y para reservar el ancho lleva un
    // doble oculto con la cifra final: se comprueba el que se lee, no los dos.
    expect(
      screen.getByText("2450", { selector: "[data-slot='animated-number-value']" }),
    ).toBeVisible();
    const search = screen.getByRole("searchbox", { name: "Buscar en la biblioteca" });
    await user.type(search, "acción cooperativa");
    expect(search).toHaveValue("acción cooperativa");

    await user.click(screen.getByRole("button", { name: "Limpiar búsqueda" }));
    expect(search).toHaveValue("");
  });

  it("aplica filtros rápidos y permite restablecerlos", async () => {
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    await user.click(screen.getByRole("button", { name: /Filtros/ }));
    const installed = screen.getByRole("combobox", { name: "Instalación" });
    await user.click(installed);
    await user.click(screen.getByRole("option", { name: "Instalados" }));
    const tracking = screen.getByRole("combobox", { name: "Seguimiento" });
    await user.click(tracking);
    await user.click(screen.getByRole("option", { name: "En seguimiento" }));

    expect(installed).toHaveTextContent("Instalados");
    expect(tracking).toHaveTextContent("En seguimiento");
    expect(screen.getByRole("button", { name: "Restablecer todos los filtros" })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Restablecer todos los filtros" }));
    expect(screen.getByRole("button", { name: "Restablecer todos los filtros" })).toBeDisabled();
  });

  it("combina filtros avanzados, resume valores false y permite quitar un solo chip", async () => {
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    await user.click(screen.getByRole("button", { name: /Filtros/ }));
    await user.click(screen.getByRole("combobox", { name: "Instalación" }));
    await user.click(screen.getByRole("option", { name: "No instalados" }));
    await user.click(screen.getByRole("combobox", { name: "Estado personal" }));
    await user.click(screen.getByRole("option", { name: "Jugando ahora" }));
    await user.type(screen.getByRole("spinbutton", { name: "Horas mínimas jugadas" }), "2");

    expect(screen.getByRole("button", { name: "Quitar filtro No instalados" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Quitar filtro Estado: Jugando ahora" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Quitar filtro Horas: desde 2 h" })).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Quitar filtro No instalados" }));
    expect(
      screen.queryByRole("button", { name: "Quitar filtro No instalados" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Quitar filtro Estado: Jugando ahora" }),
    ).toBeVisible();
  });

  it("aplica atajos inteligentes sin borrar los filtros activos", async () => {
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    await user.click(screen.getByRole("button", { name: /Filtros/ }));
    await user.click(screen.getByRole("combobox", { name: "Instalación" }));
    await user.click(screen.getByRole("option", { name: "Instalados" }));
    const preset = screen.getByRole("button", { name: "Casi terminados" });
    await user.click(preset);

    expect(preset).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Quitar filtro Instalados" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Quitar filtro Progreso: desde 75 %" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Quitar filtro Progreso: hasta 99 %" }),
    ).toBeVisible();
  });

  it("explica por qué Steam Deck no está disponible y conserva el reset visible", async () => {
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    await user.click(screen.getByRole("button", { name: /Filtros/ }));
    // La nota decía que Steam no publica el dato y que el filtro se habilitaría
    // «cuando Vindexa tenga datos verificables», sin decir de dónde iban a
    // salir: la columna llevaba vacía desde la primera migración porque nadie
    // la escribía. Ahora hay una pasada que lo pregunta, y la nota lo dice.
    expect(
      screen.getByText(/lo pregunta al informe público de la tienda por tandas/i),
    ).toBeVisible();
    expect(screen.getByRole("combobox", { name: "Compatibilidad con Steam Deck" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Restablecer todos los filtros" })).toBeVisible();
  });

  it("la vista activa se anuncia y se puede cambiar con el teclado", async () => {
    // El conmutador es un grupo de radios nativo, no tres botones sueltos: así
    // el lector de pantalla dice «1 de 3» y las flechas funcionan solas. Lo que
    // se comprueba es eso, no cómo esté hecho por dentro.
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    const grid = screen.getByRole("radio", { name: "Vista de cuadrícula" });
    const list = screen.getByRole("radio", { name: "Vista de lista" });
    const compact = screen.getByRole("radio", { name: "Vista ultracompacta" });
    expect(grid).toBeChecked();
    expect(list).not.toBeChecked();
    expect(compact).not.toBeChecked();

    await user.click(compact);
    expect(compact).toBeChecked();
    expect(grid).not.toBeChecked();

    // Las flechas mueven la selección sin tocar el ratón.
    compact.focus();
    await user.keyboard("{ArrowLeft}");
    expect(list).toBeChecked();
  });

  it("ofrece la matriz Steam-like y conserva la selección controlada", async () => {
    const user = userEvent.setup();
    render(<ToolbarHarness />);

    const sort = screen.getByRole("combobox", { name: "Ordenar biblioteca" });
    await user.click(sort);

    expect(screen.getByRole("option", { name: "Jugados recientemente" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Añadidos recientes" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Título: A–Z" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Título: Z–A" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Instalados primero" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Mayor tamaño en disco" })).toBeVisible();

    await user.click(screen.getByRole("option", { name: "Lanzamiento más reciente" }));
    expect(sort).toHaveTextContent("Lanzamiento más reciente");
  });

  /**
   * Una cifra que parece un total y es un avance.
   *
   * «Sin DRM 604» se lee como «tienes 604 juegos sin DRM». En realidad son los
   * comprobados hasta ahora: el repaso pregunta a la tienda por tandas y una
   * biblioteca grande tarda horas. La nota va en la misma línea que el número.
   */
  it("acompaña la cifra con lo que la matiza, sin taparla", async () => {
    render(
      <ToolbarHarness
        note={{ text: "2.806 sin comprobar", detail: "Vindexa pregunta por tandas." }}
      />,
    );

    const nota = screen.getByText("2.806 sin comprobar");
    expect(nota).toBeVisible();
    expect(nota).toHaveAttribute("title", "Vindexa pregunta por tandas.");
    // Y la cifra principal sigue estando: la nota acompaña, no sustituye.
    expect(
      screen.getByText("2450", { selector: "[data-slot='animated-number-value']" }),
    ).toBeVisible();
  });

  it("sin nada que matizar no escribe nada", () => {
    render(<ToolbarHarness />);
    expect(document.querySelector(".library-title__note")).toBeNull();
  });
});
