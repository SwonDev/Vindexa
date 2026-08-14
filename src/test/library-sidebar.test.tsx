import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LibrarySidebar } from "@/features/library/LibrarySidebar";
import type { AppBootstrap } from "@/lib/types";

const bootstrap = {
  stats: { totalGames: 2, installedGames: 1 },
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
  collections: [],
} as AppBootstrap;

describe("barra lateral de biblioteca", () => {
  it("colapsa Estados con semántica accesible y conserva Steam Family como alcance", async () => {
    const user = userEvent.setup();
    const onScopeChange = vi.fn();
    render(
      <TooltipProvider>
        <LibrarySidebar
          bootstrap={bootstrap}
          scope={{ kind: "all", label: "Todos los juegos" }}
          familyCount={17}
          onScopeChange={onScopeChange}
          onCreateCollection={vi.fn()}
        />
      </TooltipProvider>,
    );

    const toggle = screen.getByRole("button", { name: "ESTADOS" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: /Jugando/ })).toBeVisible();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /Jugando/ })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Steam Family/ }));
    expect(onScopeChange).toHaveBeenCalledWith({ kind: "family", label: "Steam Family" });
    expect(screen.getByText("17")).toBeVisible();
  });
});
