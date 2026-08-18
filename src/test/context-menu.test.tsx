import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { GameContextMenu, type GameContextMenuProps } from "@/components/common/GameContextMenu";
import type { CollectionSummary, GameSummary, StatusDefinition } from "@/lib/types";

const baseGame: GameSummary = {
  appId: 620,
  title: "Portal 2",
  playtimeMinutes: 640,
  playtimeRecentMinutes: 0,
  isEarlyAccess: false,
  isFree: false,
  ownershipSource: "owned",
  familyAvailability: "not_applicable",
  installed: true,
  statusId: "playing",
  statusName: "Jugando",
  statusColor: "#5caac1",
  progress: 42,
  priority: 2,
  pinned: false,
  tracking: false,
  manualPosition: 0,
};

const statuses: StatusDefinition[] = [
  { id: "playing", name: "Jugando", color: "#5caac1", position: 0, builtIn: true, gameCount: 3 },
  { id: "backlog", name: "Pendiente", color: "#82939e", position: 1, builtIn: true, gameCount: 9 },
];

const collections: CollectionSummary[] = [
  {
    id: "favoritos",
    name: "Favoritos",
    description: "",
    color: "#a4d007",
    icon: "star",
    kind: "manual",
    matchMode: "all",
    position: 0,
    gameCount: 4,
  },
  {
    id: "rejugar",
    name: "Para rejugar",
    description: "",
    color: "#5caac1",
    icon: "repeat",
    kind: "manual",
    matchMode: "all",
    position: 1,
    gameCount: 2,
  },
  {
    id: "cortos",
    name: "Cortos automáticos",
    description: "",
    color: "#d6a64b",
    icon: "clock",
    kind: "smart",
    matchMode: "all",
    position: 2,
    gameCount: 12,
  },
];

function renderMenu(overrides: Partial<GameContextMenuProps> = {}) {
  const { game, ...rest } = overrides;
  const user = userEvent.setup({ pointerEventsCheck: 0 });
  render(
    <GameContextMenu game={{ ...baseGame, ...game }} {...rest}>
      {/* El disparador debe poder recibir el foco para que Radix lo devuelva al cerrar. */}
      <article data-testid="game-card" tabIndex={-1}>
        Portal 2
      </article>
    </GameContextMenu>,
  );
  return { user, trigger: screen.getByTestId("game-card") };
}

async function openMenu(user: ReturnType<typeof userEvent.setup>, trigger: HTMLElement) {
  await user.pointer({ target: trigger, keys: "[MouseRight]" });
  return screen.findByRole("menu");
}

/**
 * Los submenús se recorren con el teclado: jsdom devuelve rectángulos vacíos y
 * el «polígono de gracia» que Radix usa para mantener abierto un submenú al
 * mover el ratón no puede calcularse, así que el puntero lo cerraría siempre.
 * Además, así se verifica la navegación accesible real.
 */
async function openSubmenu(
  user: ReturnType<typeof userEvent.setup>,
  triggerName: RegExp,
  submenuLabel: string,
) {
  const trigger = screen.getByRole("menuitem", { name: triggerName });
  await user.keyboard("{ArrowDown}");
  await waitFor(() => expect(trigger).toHaveFocus());
  await user.keyboard("{ArrowRight}");
  return screen.findByRole("menu", { name: submenuLabel });
}

describe("GameContextMenu", () => {
  it("se abre con el clic derecho y expone las acciones disponibles", async () => {
    const onPlay = vi.fn();
    const { user, trigger } = renderMenu({
      onPlay,
      onOpenDetail: vi.fn(),
      onOpenStore: vi.fn(),
      onRevealInstallation: vi.fn(),
    });

    expect(screen.queryByRole("menu")).toBeNull();

    const menu = await openMenu(user, trigger);
    expect(menu).toHaveAccessibleName("Acciones rápidas de Portal 2");
    expect(screen.getByRole("menuitem", { name: /^Jugar/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Abrir ficha/ })).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: /Abrir en la tienda oficial/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: /Revelar carpeta de instalación/ }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: /^Jugar/ }));
    expect(onPlay).toHaveBeenCalledWith(expect.objectContaining({ appId: 620 }));
  });

  it("oculta las entradas cuyo callback no llega", async () => {
    const { user, trigger } = renderMenu({ onOpenDetail: vi.fn() });

    await openMenu(user, trigger);

    expect(screen.getByRole("menuitem", { name: /Abrir ficha/ })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: /^Jugar/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Solicitar desinstalación/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Copiar título/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Estado/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Colecciones/ })).toBeNull();
  });

  it("alterna entre Jugar e Instalar según el estado de instalación", async () => {
    const onInstall = vi.fn();
    const { user, trigger } = renderMenu({
      game: { ...baseGame, installed: false },
      onPlay: vi.fn(),
      onInstall,
      onUninstall: vi.fn(),
      onRevealInstallation: vi.fn(),
    });

    await openMenu(user, trigger);

    expect(screen.queryByRole("menuitem", { name: /^Jugar/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Solicitar desinstalación/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Revelar carpeta de instalación/ })).toBeNull();

    await user.click(screen.getByRole("menuitem", { name: /^Instalar/ }));
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  it("ofrece la desinstalación como acción destructiva de un juego instalado", async () => {
    const onUninstall = vi.fn();
    const { user, trigger } = renderMenu({ onUninstall });

    await openMenu(user, trigger);
    const item = screen.getByRole("menuitem", { name: /Solicitar desinstalación/ });
    expect(item).toHaveAttribute("data-variant", "destructive");

    await user.click(item);
    expect(onUninstall).toHaveBeenCalledTimes(1);
  });

  it("cambia el estado desde el submenú y marca el estado vigente", async () => {
    const onChangeStatus = vi.fn();
    const { user, trigger } = renderMenu({ statuses, onChangeStatus });

    await openMenu(user, trigger);
    await openSubmenu(user, /Estado/, "Estado");

    const current = await screen.findByRole("menuitemradio", { name: /Jugando/ });
    expect(current).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(current).toHaveFocus());

    await user.keyboard("{ArrowDown}");
    await waitFor(() =>
      expect(screen.getByRole("menuitemradio", { name: /Pendiente/ })).toHaveFocus(),
    );
    await user.keyboard("{Enter}");

    expect(onChangeStatus).toHaveBeenCalledWith(expect.objectContaining({ appId: 620 }), "backlog");
  });

  it("cambia la prioridad entre 0 y 5 desde el submenú", async () => {
    const onChangePriority = vi.fn();
    const { user, trigger } = renderMenu({ onChangePriority });

    await openMenu(user, trigger);
    await openSubmenu(user, /Prioridad$/, "Prioridad");

    const options = await screen.findAllByRole("menuitemradio");
    expect(options.map((option) => option.textContent)).toEqual([
      "Sin prioridad",
      "Prioridad 1",
      "Prioridad 2",
      "Prioridad 3",
      "Prioridad 4",
      "Prioridad 5",
    ]);
    expect(screen.getByRole("menuitemradio", { name: "Prioridad 2" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    await waitFor(() =>
      expect(screen.getByRole("menuitemradio", { name: "Sin prioridad" })).toHaveFocus(),
    );
    await user.keyboard("{Enter}");
    expect(onChangePriority).toHaveBeenCalledWith(expect.objectContaining({ appId: 620 }), 0);
  });

  it("marca y desmarca colecciones manuales sin cerrar el submenú", async () => {
    const onToggleCollection = vi.fn();
    const { user, trigger } = renderMenu({
      collections,
      collectionIds: ["favoritos"],
      onToggleCollection,
    });

    await openMenu(user, trigger);
    await openSubmenu(user, /Colecciones/, "Colecciones");

    const favoritos = await screen.findByRole("menuitemcheckbox", { name: "Favoritos" });
    expect(favoritos).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemcheckbox", { name: "Para rejugar" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    // Las colecciones inteligentes no admiten pertenencia manual.
    expect(screen.queryByRole("menuitemcheckbox", { name: "Cortos automáticos" })).toBeNull();

    await user.keyboard("{ArrowDown}");
    await waitFor(() =>
      expect(screen.getByRole("menuitemcheckbox", { name: "Para rejugar" })).toHaveFocus(),
    );
    await user.keyboard("{Enter}");

    expect(onToggleCollection).toHaveBeenCalledWith(
      expect.objectContaining({ appId: 620 }),
      "rejugar",
      true,
    );
    // Marcar una colección no cierra el submenú: se pueden marcar varias seguidas.
    expect(screen.getByRole("menu", { name: "Colecciones" })).toBeInTheDocument();
  });

  it("invierte fijado y seguimiento según el estado actual", async () => {
    const onTogglePinned = vi.fn();
    const onToggleTracking = vi.fn();
    const { user, trigger } = renderMenu({
      game: { ...baseGame, pinned: true, tracking: false },
      onTogglePinned,
      onToggleTracking,
    });

    await openMenu(user, trigger);
    expect(screen.getByRole("menuitem", { name: /Desfijar/ })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: /Marcar seguimiento/ }));
    expect(onToggleTracking).toHaveBeenCalledWith(expect.objectContaining({ appId: 620 }), true);
  });

  it("copia título y AppID mostrando su atajo", async () => {
    const onCopyTitle = vi.fn();
    const onCopyAppId = vi.fn();
    const { user, trigger } = renderMenu({ onCopyTitle, onCopyAppId });

    await openMenu(user, trigger);
    const copyTitle = screen.getByRole("menuitem", { name: /Copiar título/ });
    expect(copyTitle.textContent).toMatch(/C$/);

    await user.click(copyTitle);
    expect(onCopyTitle).toHaveBeenCalledTimes(1);
  });

  it("escribe los atajos con los símbolos de macOS en orden canónico", async () => {
    const original = Object.getOwnPropertyDescriptor(Navigator.prototype, "userAgent");
    Object.defineProperty(navigator, "userAgent", {
      configurable: true,
      get: () => "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15",
    });

    try {
      const { user, trigger } = renderMenu({ onCopyTitle: vi.fn(), onCopyAppId: vi.fn() });
      await openMenu(user, trigger);

      expect(screen.getByRole("menuitem", { name: /Copiar título/ }).textContent).toContain("⌘C");
      expect(screen.getByRole("menuitem", { name: /Copiar AppID/ }).textContent).toContain("⇧⌘C");
    } finally {
      Object.defineProperty(navigator, "userAgent", original ?? { value: "" });
    }
  });

  it("oculta los atajos cuando la pantalla no los tiene enlazados", async () => {
    const { user, trigger } = renderMenu({ onCopyTitle: vi.fn(), showShortcuts: false });

    await openMenu(user, trigger);
    expect(screen.getByRole("menuitem", { name: "Copiar título" })).toBeInTheDocument();
  });

  it("se cierra con Escape y devuelve el foco al elemento de origen", async () => {
    const { user, trigger } = renderMenu({ onOpenDetail: vi.fn() });

    await openMenu(user, trigger);
    await user.keyboard("{Escape}");

    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    expect(document.activeElement).toBe(trigger);
  });

  it("recorre las acciones con el teclado y activa la enfocada", async () => {
    const onOpenDetail = vi.fn();
    const onOpenStore = vi.fn();
    const { user, trigger } = renderMenu({ onOpenDetail, onOpenStore });

    await openMenu(user, trigger);
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");

    expect(onOpenStore).toHaveBeenCalledTimes(1);
    expect(onOpenDetail).not.toHaveBeenCalled();
  });

  it("no se abre cuando el menú está deshabilitado", async () => {
    const { user, trigger } = renderMenu({ disabled: true, onOpenDetail: vi.fn() });

    await user.pointer({ target: trigger, keys: "[MouseRight]" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("notifica la apertura para que la pantalla sincronice la selección", async () => {
    const onOpenChange = vi.fn();
    const { user, trigger } = renderMenu({ onOpenChange, onOpenDetail: vi.fn() });

    await openMenu(user, trigger);
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });
});
