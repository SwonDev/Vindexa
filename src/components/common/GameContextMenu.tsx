import {
  IconBrandSteam,
  IconCopy,
  IconEye,
  IconEyeOff,
  IconFlag,
  IconFolderOpen,
  IconFolders,
  IconHash,
  IconInfoCircle,
  IconListNumbers,
  IconPin,
  IconPinnedOff,
  IconPlayerPlay,
  IconTrash,
} from "@tabler/icons-react";
import type * as React from "react";
import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { CollectionSummary, GameSummary, StatusDefinition } from "@/lib/types";

/**
 * Menú de acciones rápidas de un juego. Sustituye al menú contextual nativo del
 * webview: se abre con clic derecho sobre cualquier tarjeta o fila y solo
 * muestra las entradas cuya acción ha llegado por props, de modo que nunca hay
 * opciones muertas.
 */

/** Prioridades admitidas por `GameSummary.priority`. */
export const GAME_PRIORITIES: readonly number[] = [0, 1, 2, 3, 4, 5];

export type GameContextAction =
  | "play"
  | "install"
  | "uninstall"
  | "detail"
  | "store"
  | "reveal"
  | "pin"
  | "tracking"
  | "copyTitle"
  | "copyAppId";

/**
 * Etiquetas de atajo que muestra el menú. Se exportan para que la pantalla que
 * lo monta pueda enlazar exactamente las mismas combinaciones de teclado y no
 * se anuncie un atajo que no existe.
 */
export const DEFAULT_GAME_CONTEXT_SHORTCUTS: Readonly<Partial<Record<GameContextAction, string>>> =
  {
    play: "Intro",
    install: "Intro",
    detail: "Mod+I",
    store: "Mod+T",
    reveal: "Mod+Shift+R",
    pin: "Mod+D",
    tracking: "Mod+E",
    copyTitle: "Mod+C",
    copyAppId: "Mod+Shift+C",
  };

/**
 * Una acción que sólo tiene sentido en la pantalla que monta el menú.
 *
 * «Quitar de la colección» no existe en la biblioteca y «Quitar del
 * planificador» no existe en una colección: son verbos del sitio, no del juego.
 * Entran aquí en vez de crecer la lista de props con una opción por pantalla.
 */
export interface GameContextExtraAction {
  id: string;
  label: string;
  icon?: React.ReactNode | undefined;
  /** Se pinta en rojo y se separa: quita algo, aunque no borre el juego. */
  destructive?: boolean | undefined;
  disabled?: boolean | undefined;
  onSelect: (game: GameSummary) => void;
}

export interface GameContextMenuProps {
  /** Juego sobre el que actúan todas las entradas del menú. */
  game: GameSummary;
  /** Elemento que dispara el menú con el clic derecho. */
  children: React.ReactNode;
  /** Fusiona el disparador con el hijo en lugar de envolverlo en un `span`. */
  asChild?: boolean | undefined;
  /** Desactiva por completo el menú (por ejemplo, durante un arrastre). */
  disabled?: boolean | undefined;
  /** Bloquea las acciones que hablan con Steam mientras hay una en curso. */
  busy?: boolean | undefined;
  /** Estados disponibles para el submenú «Estado». */
  statuses?: readonly StatusDefinition[] | undefined;
  /** Colecciones manuales disponibles para el submenú «Colecciones». */
  collections?: readonly CollectionSummary[] | undefined;
  /** Colecciones a las que ya pertenece el juego. */
  collectionIds?: readonly string[] | undefined;
  /** Sustituye o amplía las etiquetas de atajo mostradas. */
  shortcuts?: Partial<Record<GameContextAction, string>> | undefined;
  /** Oculta las etiquetas de atajo si la pantalla no las tiene enlazadas. */
  showShortcuts?: boolean | undefined;
  /** Acciones propias de la pantalla que monta el menú. */
  extraActions?: readonly GameContextExtraAction[] | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  onPlay?: ((game: GameSummary) => void) | undefined;
  onInstall?: ((game: GameSummary) => void) | undefined;
  onUninstall?: ((game: GameSummary) => void) | undefined;
  onOpenDetail?: ((game: GameSummary) => void) | undefined;
  onOpenStore?: ((game: GameSummary) => void) | undefined;
  onRevealInstallation?: ((game: GameSummary) => void) | undefined;
  onChangeStatus?: ((game: GameSummary, statusId: string) => void) | undefined;
  onChangePriority?: ((game: GameSummary, priority: number) => void) | undefined;
  onToggleCollection?:
    | ((game: GameSummary, collectionId: string, member: boolean) => void)
    | undefined;
  onTogglePinned?: ((game: GameSummary, pinned: boolean) => void) | undefined;
  onToggleTracking?: ((game: GameSummary, tracking: boolean) => void) | undefined;
  onCopyTitle?: ((game: GameSummary) => void) | undefined;
  onCopyAppId?: ((game: GameSummary) => void) | undefined;
}

/** Símbolos y orden canónico de los modificadores en macOS. */
const APPLE_MODIFIERS: Readonly<Record<string, string>> = {
  Ctrl: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Mod: "⌘",
};
const APPLE_MODIFIER_ORDER = ["⌃", "⌥", "⇧", "⌘"];

function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac|iphone|ipad/i.test(`${navigator.userAgent} ${navigator.platform}`);
}

/**
 * Convierte «Mod+Shift+C» en la etiqueta de la plataforma: «⇧⌘C» en macOS y
 * «Ctrl+Shift+C» en el resto, respetando el orden canónico de Apple.
 */
function formatShortcut(binding: string | undefined): string | undefined {
  if (!binding) return undefined;
  const parts = binding.split("+");
  if (!isApplePlatform()) {
    return parts.map((part) => (part === "Mod" ? "Ctrl" : part)).join("+");
  }
  const modifiers: string[] = [];
  const keys: string[] = [];
  for (const part of parts) {
    const symbol = APPLE_MODIFIERS[part];
    if (symbol) modifiers.push(symbol);
    else keys.push(part);
  }
  modifiers.sort(
    (left, right) => APPLE_MODIFIER_ORDER.indexOf(left) - APPLE_MODIFIER_ORDER.indexOf(right),
  );
  return [...modifiers, ...keys].join("");
}

function priorityLabel(priority: number): string {
  return priority === 0 ? "Sin prioridad" : `Prioridad ${priority}`;
}

export function GameContextMenu({
  game,
  children,
  asChild = true,
  disabled = false,
  busy = false,
  statuses,
  collections,
  collectionIds,
  shortcuts,
  showShortcuts = true,
  extraActions,
  onOpenChange,
  onPlay,
  onInstall,
  onUninstall,
  onOpenDetail,
  onOpenStore,
  onRevealInstallation,
  onChangeStatus,
  onChangePriority,
  onToggleCollection,
  onTogglePinned,
  onToggleTracking,
  onCopyTitle,
  onCopyAppId,
}: GameContextMenuProps) {
  const bindings = { ...DEFAULT_GAME_CONTEXT_SHORTCUTS, ...shortcuts };
  const shortcut = (action: GameContextAction) =>
    showShortcuts ? formatShortcut(bindings[action]) : undefined;

  const memberships = new Set(collectionIds ?? []);
  const manualCollections = (collections ?? []).filter(
    (collection) => collection.kind === "manual",
  );

  const showLaunchGroup = Boolean(
    (game.installed && onPlay) || (!game.installed && onInstall) || (game.installed && onUninstall),
  );
  const showNavigationGroup = Boolean(
    onOpenDetail || onOpenStore || (game.installed && onRevealInstallation),
  );
  const showStatusSub = Boolean(onChangeStatus && (statuses?.length ?? 0) > 0);
  const showPrioritySub = Boolean(onChangePriority);
  const showCollectionsSub = Boolean(onToggleCollection && manualCollections.length > 0);
  const showOrganizeGroup = showStatusSub || showPrioritySub || showCollectionsSub;
  const showFlagsGroup = Boolean(onTogglePinned || onToggleTracking);
  const showCopyGroup = Boolean(onCopyTitle || onCopyAppId);
  const showExtraGroup = (extraActions?.length ?? 0) > 0;

  return (
    <ContextMenu {...(onOpenChange ? { onOpenChange } : {})}>
      <ContextMenuTrigger asChild={asChild} disabled={disabled}>
        {children}
      </ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${game.title}`}>
        <ContextMenuLabel>{game.title}</ContextMenuLabel>

        {showLaunchGroup && (
          <ContextMenuGroup>
            {game.installed && onPlay && (
              <ContextMenuItem disabled={busy} onSelect={() => onPlay(game)}>
                <IconPlayerPlay aria-hidden="true" />
                Jugar
                {shortcut("play") && <ContextMenuShortcut>{shortcut("play")}</ContextMenuShortcut>}
              </ContextMenuItem>
            )}
            {!game.installed && onInstall && (
              <ContextMenuItem disabled={busy} onSelect={() => onInstall(game)}>
                <IconBrandSteam aria-hidden="true" />
                Instalar
                {shortcut("install") && (
                  <ContextMenuShortcut>{shortcut("install")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
            {game.installed && onUninstall && (
              <ContextMenuItem
                variant="destructive"
                disabled={busy}
                onSelect={() => onUninstall(game)}
              >
                <IconTrash aria-hidden="true" />
                Solicitar desinstalación
              </ContextMenuItem>
            )}
          </ContextMenuGroup>
        )}

        {showLaunchGroup && showNavigationGroup && <ContextMenuSeparator />}

        {showNavigationGroup && (
          <ContextMenuGroup>
            {onOpenDetail && (
              <ContextMenuItem onSelect={() => onOpenDetail(game)}>
                <IconInfoCircle aria-hidden="true" />
                Abrir ficha
                {shortcut("detail") && (
                  <ContextMenuShortcut>{shortcut("detail")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
            {onOpenStore && (
              <ContextMenuItem disabled={busy} onSelect={() => onOpenStore(game)}>
                <IconBrandSteam aria-hidden="true" />
                Abrir en la tienda oficial
                {shortcut("store") && (
                  <ContextMenuShortcut>{shortcut("store")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
            {game.installed && onRevealInstallation && (
              <ContextMenuItem disabled={busy} onSelect={() => onRevealInstallation(game)}>
                <IconFolderOpen aria-hidden="true" />
                Revelar carpeta de instalación
                {shortcut("reveal") && (
                  <ContextMenuShortcut>{shortcut("reveal")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
          </ContextMenuGroup>
        )}

        {(showLaunchGroup || showNavigationGroup) && showExtraGroup && <ContextMenuSeparator />}

        {showExtraGroup && (
          <ContextMenuGroup>
            {(extraActions ?? []).map((action) => (
              <ContextMenuItem
                key={action.id}
                disabled={busy || action.disabled === true}
                {...(action.destructive ? { variant: "destructive" as const } : {})}
                onSelect={() => action.onSelect(game)}
              >
                {action.icon}
                {action.label}
              </ContextMenuItem>
            ))}
          </ContextMenuGroup>
        )}

        {(showNavigationGroup || showExtraGroup) && showOrganizeGroup && <ContextMenuSeparator />}

        {showStatusSub && onChangeStatus && (
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <IconFlag aria-hidden="true" />
              Estado
            </ContextMenuSubTrigger>
            <ContextMenuSubContent>
              <ContextMenuRadioGroup
                value={game.statusId}
                onValueChange={(statusId) => onChangeStatus(game, statusId)}
              >
                {(statuses ?? []).map((status) => (
                  <ContextMenuRadioItem key={status.id} value={status.id}>
                    <span
                      aria-hidden="true"
                      className="size-2 shrink-0 rounded-[1px]"
                      style={{ backgroundColor: status.color }}
                    />
                    {status.name}
                  </ContextMenuRadioItem>
                ))}
              </ContextMenuRadioGroup>
            </ContextMenuSubContent>
          </ContextMenuSub>
        )}

        {showPrioritySub && onChangePriority && (
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <IconListNumbers aria-hidden="true" />
              Prioridad
            </ContextMenuSubTrigger>
            <ContextMenuSubContent>
              <ContextMenuRadioGroup
                value={String(game.priority)}
                onValueChange={(value) => onChangePriority(game, Number(value))}
              >
                {GAME_PRIORITIES.map((priority) => (
                  <ContextMenuRadioItem key={priority} value={String(priority)}>
                    {priorityLabel(priority)}
                  </ContextMenuRadioItem>
                ))}
              </ContextMenuRadioGroup>
            </ContextMenuSubContent>
          </ContextMenuSub>
        )}

        {showCollectionsSub && onToggleCollection && (
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <IconFolders aria-hidden="true" />
              Colecciones
            </ContextMenuSubTrigger>
            <ContextMenuSubContent>
              {manualCollections.map((collection) => (
                <ContextMenuCheckboxItem
                  key={collection.id}
                  checked={memberships.has(collection.id)}
                  onSelect={(event) => event.preventDefault()}
                  onCheckedChange={(checked) =>
                    onToggleCollection(game, collection.id, checked === true)
                  }
                >
                  {collection.name}
                </ContextMenuCheckboxItem>
              ))}
            </ContextMenuSubContent>
          </ContextMenuSub>
        )}

        {showOrganizeGroup && showFlagsGroup && <ContextMenuSeparator />}

        {showFlagsGroup && (
          <ContextMenuGroup>
            {onTogglePinned && (
              <ContextMenuItem onSelect={() => onTogglePinned(game, !game.pinned)}>
                {game.pinned ? (
                  <IconPinnedOff aria-hidden="true" />
                ) : (
                  <IconPin aria-hidden="true" />
                )}
                {game.pinned ? "Desfijar" : "Fijar"}
                {shortcut("pin") && <ContextMenuShortcut>{shortcut("pin")}</ContextMenuShortcut>}
              </ContextMenuItem>
            )}
            {onToggleTracking && (
              <ContextMenuItem onSelect={() => onToggleTracking(game, !game.tracking)}>
                {game.tracking ? <IconEyeOff aria-hidden="true" /> : <IconEye aria-hidden="true" />}
                {game.tracking ? "Quitar seguimiento" : "Marcar seguimiento"}
                {shortcut("tracking") && (
                  <ContextMenuShortcut>{shortcut("tracking")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
          </ContextMenuGroup>
        )}

        {showFlagsGroup && showCopyGroup && <ContextMenuSeparator />}

        {showCopyGroup && (
          <ContextMenuGroup>
            {onCopyTitle && (
              <ContextMenuItem onSelect={() => onCopyTitle(game)}>
                <IconCopy aria-hidden="true" />
                Copiar título
                {shortcut("copyTitle") && (
                  <ContextMenuShortcut>{shortcut("copyTitle")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
            {onCopyAppId && (
              <ContextMenuItem onSelect={() => onCopyAppId(game)}>
                <IconHash aria-hidden="true" />
                Copiar AppID
                {shortcut("copyAppId") && (
                  <ContextMenuShortcut>{shortcut("copyAppId")}</ContextMenuShortcut>
                )}
              </ContextMenuItem>
            )}
          </ContextMenuGroup>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}
