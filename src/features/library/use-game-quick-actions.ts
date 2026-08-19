import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";
import type { GameContextMenuProps } from "@/components/common/GameContextMenu";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppBootstrap, GameSummary, UpdateGameInput } from "@/lib/types";

/**
 * Las acciones rápidas de un juego, iguales se abran donde se abran.
 *
 * # Por qué existe
 *
 * El clic derecho sobre un juego tiene sentido en la biblioteca, dentro de una
 * colección, en una tarjeta del planificador, en una lista curada y en la lista
 * de seguimiento: es el mismo juego y son las mismas acciones. Escribirlas cinco
 * veces significa que la sexta se olvida y que el día que cambie una regla —qué
 * mensaje se enseña, qué se invalida— cambie sólo en cuatro sitios.
 *
 * Aquí viven una vez. Cada pantalla aporta lo suyo: dónde se cuenta lo que ha
 * pasado (`onMessage`) y, si mantiene una pila de deshacer, qué apuntar antes de
 * tocar nada (`onBeforeChange`).
 *
 * # Qué no hace
 *
 * No desinstala. Desinstalar abre el cliente oficial y se confirma antes, así
 * que vive en la pantalla que tiene esa confirmación. Ofrecerlo desde un menú
 * rápido sin confirmar sería la única acción de aquí que no se puede deshacer.
 */

export interface GameQuickActionsOptions {
  /** Estados y colecciones que ofrecerá el menú. */
  bootstrap?: AppBootstrap | undefined;
  /** Dónde contar lo que ha pasado; sin esto, los cambios ocurren en silencio. */
  onMessage?: ((message: string) => void) | undefined;
  /** Se llama justo antes de cambiar algo, para quien lleve pila de deshacer. */
  onBeforeChange?: ((game: GameSummary, label: string) => void) | undefined;
  /** Abrir la ficha, que cada pantalla resuelve a su manera. */
  onOpenDetail?: ((game: GameSummary) => void) | undefined;
}

/** Lo que se le pasa a `GameContextMenu`, ya cableado. */
export type GameQuickActions = Pick<
  GameContextMenuProps,
  | "statuses"
  | "collections"
  | "collectionIds"
  | "onOpenDetail"
  | "onOpenStore"
  | "onPlay"
  | "onInstall"
  | "onRevealInstallation"
  | "onChangeStatus"
  | "onChangePriority"
  | "onToggleCollection"
  | "onTogglePinned"
  | "onToggleTracking"
  | "onCopyTitle"
  | "onCopyAppId"
> & {
  /** Hay una operación en vuelo: el menú desactiva lo que habla con la tienda. */
  busy: boolean;
  /** Colecciones a las que pertenece este juego, para el submenú de colecciones. */
  collectionIdsOf: (game: GameSummary) => readonly string[];
};

/**
 * El estado personal vigente con un parche encima.
 *
 * `update_game` escribe la fila entera, así que hay que mandar todo lo que ya
 * había: enviar sólo el campo tocado borraría las notas, el progreso o la fecha
 * objetivo sin que nadie lo pidiera.
 */
export function personalUpdate(
  game: GameSummary,
  patch: Partial<UpdateGameInput>,
): UpdateGameInput {
  return {
    appId: game.appId,
    statusId: game.statusId,
    progress: game.progress,
    priority: game.priority,
    pinned: game.pinned,
    tracking: game.tracking,
    rating: game.rating,
    estimatedMinutes: game.estimatedMinutes,
    targetDate: game.targetDate,
    nextAction: game.nextAction,
    checkpoint: game.checkpoint,
    notes: game.notes,
    ...patch,
  };
}

export function useGameQuickActions(options: GameQuickActionsOptions = {}): GameQuickActions {
  const { bootstrap, onMessage, onBeforeChange, onOpenDetail } = options;
  const queryClient = useQueryClient();

  const contar = useCallback((message: string) => onMessage?.(message), [onMessage]);
  const apuntar = useCallback(
    (game: GameSummary, label: string) => onBeforeChange?.(game, label),
    [onBeforeChange],
  );

  const organizar = useMutation({
    mutationFn: ({ input }: { input: UpdateGameInput; message: string }) => api.updateGame(input),
    onSuccess: (_data, variables) => {
      contar(variables.message);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => contar(getErrorMessage(cause)),
  });
  const colecciones = useMutation({
    mutationFn: ({
      appId,
      collectionIds,
    }: {
      appId: number;
      collectionIds: string[];
      message: string;
    }) => api.setGameCollections(appId, collectionIds),
    onSuccess: (_data, variables) => {
      contar(variables.message);
      void queryClient.invalidateQueries();
    },
    onError: (cause) => contar(getErrorMessage(cause)),
  });
  const tienda = useMutation({
    mutationFn: (game: GameSummary) => api.openStore(game.appId),
    onSuccess: (_data, game) =>
      contar(`La tienda oficial de ${game.title} se abrió en una sesión privada.`),
    onError: (cause) => contar(getErrorMessage(cause)),
  });
  const jugar = useMutation({
    mutationFn: (game: GameSummary) => api.launchGame(game.appId),
    onSuccess: (_data, game) => contar(`Steam recibió la orden de abrir ${game.title}.`),
    onError: (cause) => contar(getErrorMessage(cause)),
  });
  const instalar = useMutation({
    mutationFn: (game: GameSummary) => api.installGame(game.appId),
    onSuccess: (_data, game) => contar(`Steam recibió la solicitud para instalar ${game.title}.`),
    onError: (cause) => contar(getErrorMessage(cause)),
  });
  const revelar = useMutation({
    mutationFn: (game: GameSummary) => api.revealInstallation(game.appId),
    onError: (cause) => contar(getErrorMessage(cause)),
  });

  const busy =
    organizar.isPending ||
    colecciones.isPending ||
    tienda.isPending ||
    jugar.isPending ||
    instalar.isPending;

  return useMemo<GameQuickActions>(
    () => ({
      busy,
      statuses: bootstrap?.statuses,
      collections: bootstrap?.collections,
      collectionIdsOf: (game) => game.collectionIds ?? [],
      ...(onOpenDetail ? { onOpenDetail } : {}),
      onOpenStore: (game) => tienda.mutate(game),
      onPlay: (game) => jugar.mutate(game),
      onInstall: (game) => instalar.mutate(game),
      onRevealInstallation: (game) => revelar.mutate(game),
      onChangeStatus: (game, statusId) => {
        const status = bootstrap?.statuses.find((candidate) => candidate.id === statusId);
        apuntar(game, `${game.title} pasó a «${status?.name ?? statusId}»`);
        organizar.mutate({
          input: personalUpdate(game, { statusId }),
          message: `${game.title} pasó a «${status?.name ?? statusId}».`,
        });
      },
      onChangePriority: (game, priority) => {
        apuntar(game, `prioridad de ${game.title}`);
        organizar.mutate({
          input: personalUpdate(game, { priority }),
          message: `Prioridad de ${game.title} fijada en ${priority}.`,
        });
      },
      onTogglePinned: (game, pinned) => {
        apuntar(game, `${game.title} ${pinned ? "fijado" : "desfijado"}`);
        organizar.mutate({
          input: personalUpdate(game, { pinned }),
          message: pinned
            ? `${game.title} fijado en la biblioteca.`
            : `${game.title} ya no está fijado.`,
        });
      },
      onToggleTracking: (game, tracking) => {
        apuntar(game, `seguimiento de ${game.title}`);
        organizar.mutate({
          input: personalUpdate(game, { tracking }),
          message: tracking
            ? `${game.title} añadido a seguimiento.`
            : `${game.title} salió de seguimiento.`,
        });
      },
      onToggleCollection: (game, collectionId, member) => {
        const current = game.collectionIds ?? [];
        const next = member
          ? Array.from(new Set([...current, collectionId]))
          : current.filter((id) => id !== collectionId);
        const collection = bootstrap?.collections.find(
          (candidate) => candidate.id === collectionId,
        );
        colecciones.mutate({
          appId: game.appId,
          collectionIds: next,
          message: member
            ? `${game.title} añadido a «${collection?.name ?? "la colección"}».`
            : `${game.title} retirado de «${collection?.name ?? "la colección"}».`,
        });
      },
      onCopyTitle: (game) => {
        navigator.clipboard?.writeText(game.title).catch(() => undefined);
      },
      onCopyAppId: (game) => {
        navigator.clipboard?.writeText(String(game.appId)).catch(() => undefined);
      },
    }),
    [
      apuntar,
      bootstrap,
      busy,
      colecciones,
      instalar,
      jugar,
      onOpenDetail,
      organizar,
      revelar,
      tienda,
    ],
  );
}
