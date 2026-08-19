import {
  IconAlertTriangle,
  IconBrandSteam,
  IconCopy,
  IconDeviceGamepad2,
  IconDownload,
  IconFilterCheck,
  IconLogout,
  IconPlayerPlay,
} from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useRef, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { EmptyState } from "@/components/common/EmptyState";
import { LoadingState } from "@/components/common/LoadingState";
import { ProgressMeter } from "@/components/common/ProgressMeter";
import { useReducedMotion } from "@/components/motion/use-reduced-motion";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  clampCouchIndex,
  couchColumns,
  moveCouchFocus,
  pageCouchFocus,
} from "@/features/couch/couch-grid";
import { detectHostPlatform, platformWarning } from "@/features/library/game-platforms";
import { type GamepadSignal, useGamepad } from "@/hooks/use-gamepad";
import { formatDate, formatPlaytime, formatSteamDeckStatus } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { GameSummary } from "@/lib/types";
import "./couch.css";

/**
 * Modo sofá.
 *
 * Es la biblioteca vista desde el otro extremo de la habitación: pantalla
 * completa, tipografía grande, foco inequívoco y todo alcanzable con un mando.
 * Nada de lo que hace es exclusivo del mando —teclado y ratón recorren la
 * misma rejilla— porque un modo que sólo responde al mando deja de funcionar
 * cuando se acaban las pilas.
 *
 * El foco es el foco real del DOM. El mando no pinta un recuadro por su cuenta:
 * llama a `focus()` sobre la carátula que toca, de modo que un lector de
 * pantalla puede seguir el recorrido y la vista se desplaza sola detrás.
 */

/**
 * Juegos que se traen de una vez. El modo sofá se recorre carátula a carátula,
 * así que una página larga vale más que la paginación infinita del escritorio:
 * con este techo caben unas sesenta filas sin volver a hablar con SQLite.
 */
const COUCH_GAME_LIMIT = 240;

interface CouchHint {
  /** Botón del mapeo estándar, con la nomenclatura de un mando de Xbox. */
  button: string;
  keyboard: string;
  label: string;
}

const HINTS: readonly CouchHint[] = [
  { button: "A", keyboard: "Intro", label: "Jugar" },
  { button: "B", keyboard: "Esc", label: "Salir" },
  { button: "X", keyboard: "T", label: "Tienda" },
  { button: "Y", keyboard: "I", label: "Sólo instalados" },
  { button: "LB · RB", keyboard: "Re Pág · Av Pág", label: "Saltar filas" },
  { button: "Cruceta · stick", keyboard: "Flechas", label: "Mover el foco" },
];

export interface CouchScreenProps {
  /** Devuelve la aplicación a su cromado normal. */
  onExit: () => void;
}

export function CouchScreen({ onExit }: CouchScreenProps) {
  const [installedOnly, setInstalledOnly] = useState(false);
  const [focusIndex, setFocusIndex] = useState(0);
  const [gridWidth, setGridWidth] = useState(0);
  const [announcement, setAnnouncement] = useState("");
  const [actionFailed, setActionFailed] = useState(false);
  const gridRef = useRef<HTMLUListElement>(null);
  const tiles = useRef(new Map<number, HTMLButtonElement>());
  const reducedMotion = useReducedMotion();

  const gamesQuery = useQuery({
    queryKey: ["couch-games", installedOnly],
    queryFn: () =>
      api.listGames({
        // Lo último jugado arriba: quien se sienta en el sofá suele venir a
        // seguir algo, no a redescubrir el fondo del catálogo.
        sort: "lastPlayed",
        limit: COUCH_GAME_LIMIT,
        ...(installedOnly ? { installed: true } : {}),
      }),
    staleTime: 30_000,
  });
  const games: readonly GameSummary[] = gamesQuery.data?.items ?? [];
  const total = games.length;
  /**
   * Lo que hay en la biblioteca, que no es lo mismo que lo que cabe en esta
   * página. Decir «240 juegos» con mil ochocientos detrás sería falso.
   */
  const catalogTotal = gamesQuery.data?.total ?? total;
  const focused = games[clampCouchIndex(focusIndex, total)];

  /**
   * La ficha necesita datos que el listado no trae —descripción, estudio,
   * géneros—. Se pide sólo del juego enfocado y se descarta si llega tarde:
   * enseñar la sinopsis del juego anterior sobre la carátula del siguiente es
   * peor que no enseñar ninguna.
   */
  const focusedAppId = focused?.appId;
  const detailQuery = useQuery({
    // Misma clave que la ficha del escritorio: abrir el modo sofá no vuelve a
    // pedir a SQLite lo que ya está en caché.
    queryKey: ["game", focusedAppId],
    queryFn: () => api.gameDetail(focusedAppId as number),
    enabled: focusedAppId !== undefined,
    staleTime: 60_000,
  });
  const detail = detailQuery.data?.appId === focusedAppId ? detailQuery.data : undefined;

  const columns = couchColumns(gridWidth);

  useEffect(() => {
    setFocusIndex((index) => clampCouchIndex(index, total));
  }, [total]);

  useEffect(() => {
    const grid = gridRef.current;
    if (!grid) return;
    const measure = () => setGridWidth(grid.clientWidth);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(grid);
    return () => observer.disconnect();
  }, []);

  // El foco del DOM sigue al índice, y el desplazamiento al foco. `focus()` no
  // desplaza por su cuenta para que la animación la decida `scrollIntoView`,
  // que es la que respeta la preferencia de movimiento reducido.
  useEffect(() => {
    if (!focused) return;
    const tile = tiles.current.get(focused.appId);
    if (!tile) return;
    tile.focus({ preventScroll: true });
    tile.scrollIntoView({ block: "nearest", behavior: reducedMotion ? "auto" : "smooth" });
  }, [focused, reducedMotion]);

  const report = (message: string, failed = false) => {
    setAnnouncement(message);
    setActionFailed(failed);
  };

  const runAction = (pending: string, success: string, task: () => Promise<unknown>) => {
    report(pending);
    void task()
      .then(() => report(success))
      .catch((cause: unknown) => report(getErrorMessage(cause), true));
  };

  /* Toma el juego como argumento en vez de leer el enfocado: el menú
     contextual actúa sobre la carátula sobre la que se pulsó, que puede no ser
     la que tiene el foco todavía. */
  const playGame = (game: GameSummary) => {
    // La guarda vive aquí y no sólo en el botón porque el mando y el teclado
    // llaman a esta función directamente: esconder el control no basta.
    if (!game.installed) {
      const aviso = platformWarning(game, hostPlatform);
      if (aviso) {
        setAnnouncement(aviso);
        return;
      }
    }
    if (game.installed) {
      runAction(
        `Abriendo ${game.title}…`,
        `Steam recibió la solicitud para iniciar ${game.title}.`,
        () => api.launchGame(game.appId),
      );
      return;
    }
    runAction(
      `Preparando la instalación de ${game.title}…`,
      `Steam recibió la solicitud para instalar ${game.title}.`,
      () => api.installGame(game.appId),
    );
  };

  const playFocused = () => {
    if (!focused) return;
    playGame(focused);
  };

  const openStoreFor = (game: GameSummary) =>
    runAction(
      "Abriendo la tienda protegida…",
      `La tienda oficial de ${game.title} se abrió en una sesión privada.`,
      () => api.openStore(game.appId),
    );

  const openStore = () => {
    if (!focused) return;
    openStoreFor(focused);
  };

  const toggleInstalledOnly = () => {
    setInstalledOnly((only) => {
      report(only ? "Mostrando toda la biblioteca." : "Mostrando sólo los juegos instalados.");
      return !only;
    });
    setFocusIndex(0);
  };

  const move = (direction: "up" | "down" | "left" | "right") => {
    setFocusIndex((index) => moveCouchFocus(index, total, columns, direction));
  };

  const page = (direction: "up" | "down") => {
    setFocusIndex((index) => pageCouchFocus(index, total, columns, direction));
  };

  const handleSignal = (signal: GamepadSignal) => {
    if (signal.kind === "direction") {
      move(signal.direction);
      return;
    }
    switch (signal.button) {
      case "accept":
        playFocused();
        return;
      case "cancel":
      case "start":
        onExit();
        return;
      case "alternate":
        openStore();
        return;
      case "context":
        toggleInstalledOnly();
        return;
      case "leftShoulder":
        page("up");
        return;
      case "rightShoulder":
        page("down");
        return;
      default:
        return;
    }
  };

  const gamepad = useGamepad({ onSignal: handleSignal });

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.defaultPrevented || event.altKey || event.metaKey || event.ctrlKey) return;
    switch (event.key) {
      case "ArrowUp":
      case "ArrowDown":
      case "ArrowLeft":
      case "ArrowRight": {
        event.preventDefault();
        move(
          event.key === "ArrowUp"
            ? "up"
            : event.key === "ArrowDown"
              ? "down"
              : event.key === "ArrowLeft"
                ? "left"
                : "right",
        );
        return;
      }
      case "PageUp":
        event.preventDefault();
        page("up");
        return;
      case "PageDown":
        event.preventDefault();
        page("down");
        return;
      case "Home":
        event.preventDefault();
        setFocusIndex(0);
        return;
      case "End":
        event.preventDefault();
        setFocusIndex(clampCouchIndex(total - 1, total));
        return;
      case "Enter":
        // El teclado ejecuta la acción principal, no el clic de la carátula:
        // pulsar Intro sobre un juego tiene que jugar, igual que la A.
        event.preventDefault();
        playFocused();
        return;
      case "Escape":
        event.preventDefault();
        onExit();
        return;
      case "t":
      case "T":
        event.preventDefault();
        openStore();
        return;
      case "i":
      case "I":
        event.preventDefault();
        toggleInstalledOnly();
        return;
      default:
        return;
    }
  };

  /**
   * Sistema real de este equipo. Ofrecer «Instalar» un juego que la tienda no
   * publica para él sería prometer algo que va a fallar.
   */
  const hostPlatform = detectHostPlatform(navigator.userAgent);
  const incompatibleNote = focused ? platformWarning(focused, hostPlatform) : undefined;

  const controllerNote = !gamepad.supported
    ? "Este entorno no expone la API de mandos: usa el teclado o el ratón."
    : !gamepad.connected
      ? "Sin mando conectado. Pulsa un botón para que el sistema lo detecte, o usa el teclado."
      : gamepad.standardMapping
        ? `Mando conectado: ${gamepad.id ?? "sin nombre"}.`
        : `${gamepad.id ?? "El mando"} no declara el mapeo estándar: los botones pueden no coincidir. El teclado sigue funcionando.`;

  return (
    <section
      className="couch"
      aria-label="Modo sofá"
      // El modo sofá se dibuja fuera de `.app-shell`, así que declara la
      // plataforma por su cuenta: la cabecera necesita saber si tiene que
      // dejarle sitio a los botones de la ventana.
      data-platform={/Macintosh|Mac OS X/.test(navigator.userAgent) ? "macos" : "other"}
      onKeyDown={handleKeyDown}
    >
      <header className="couch__header">
        <div className="couch__identity">
          <IconDeviceGamepad2 aria-hidden="true" size={26} stroke={1.7} />
          <div>
            <h1>Modo sofá</h1>
            <p>
              {installedOnly ? "Sólo instalados" : "Toda la biblioteca"} ·{" "}
              {catalogTotal > total
                ? `${total.toLocaleString("es-ES")} de ${catalogTotal.toLocaleString("es-ES")}, los más recientes`
                : `${total.toLocaleString("es-ES")} juego${total === 1 ? "" : "s"}`}
            </p>
          </div>
        </div>
        <div className="couch__header-actions">
          <button
            type="button"
            className="couch-action"
            data-active={installedOnly ? "true" : undefined}
            aria-pressed={installedOnly}
            aria-label="Mostrar sólo los juegos instalados"
            onClick={toggleInstalledOnly}
          >
            <IconFilterCheck aria-hidden="true" size={22} stroke={1.7} />
            Sólo instalados
          </button>
          <button
            type="button"
            className="couch-action"
            aria-label="Salir del modo sofá"
            onClick={onExit}
          >
            <IconLogout aria-hidden="true" size={22} stroke={1.7} />
            Salir
          </button>
        </div>
      </header>

      <p
        className="couch__controller"
        data-warning={gamepad.connected && !gamepad.standardMapping ? "true" : undefined}
      >
        {gamepad.connected && !gamepad.standardMapping ? (
          <IconAlertTriangle aria-hidden="true" size={18} stroke={1.8} />
        ) : null}
        {controllerNote}
      </p>

      <div className="couch__body">
        {gamesQuery.isPending ? (
          <LoadingState label="Preparando el modo sofá" />
        ) : gamesQuery.isError ? (
          <div className="couch__error" role="alert">
            <h2>No se pudo cargar la biblioteca</h2>
            <p>{getErrorMessage(gamesQuery.error)}</p>
            <button
              type="button"
              className="couch-action"
              onClick={() => void gamesQuery.refetch()}
            >
              Reintentar
            </button>
          </div>
        ) : total === 0 ? (
          <EmptyState
            icon={IconDeviceGamepad2}
            title={installedOnly ? "No hay juegos instalados" : "La biblioteca está vacía"}
            description={
              installedOnly
                ? "Quita el filtro para ver toda la biblioteca o instala algo desde Steam."
                : "Sincroniza con Steam desde la biblioteca para traer tus juegos."
            }
            action={
              installedOnly ? (
                <button type="button" className="couch-action" onClick={toggleInstalledOnly}>
                  Ver toda la biblioteca
                </button>
              ) : undefined
            }
          />
        ) : (
          <>
            <ul
              className="couch__grid"
              ref={gridRef}
              aria-label="Juegos de la biblioteca"
              style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
            >
              {games.map((game, index) => (
                <li key={game.appId}>
                  {/* El modo salón se lleva con mando, pero también se abre con
                      ratón: el clic derecho ofrece lo mismo que los botones de
                      la ficha lateral, sobre la carátula que se señala. */}
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <button
                        type="button"
                        className="couch-tile"
                        ref={(node) => {
                          if (node) tiles.current.set(game.appId, node);
                          else tiles.current.delete(game.appId);
                        }}
                        // Foco itinerante: sólo la carátula enfocada entra en el
                        // recorrido del tabulador, como en cualquier rejilla.
                        tabIndex={focused?.appId === game.appId ? 0 : -1}
                        aria-current={focused?.appId === game.appId ? "true" : undefined}
                        aria-label={`${game.title}. ${game.statusName}. ${game.installed ? "Instalado" : "No instalado"}. ${formatPlaytime(game.playtimeMinutes)} jugados`}
                        onFocus={() => setFocusIndex(index)}
                        onClick={() => setFocusIndex(index)}
                        onDoubleClick={playFocused}
                      >
                        <Artwork
                          appId={game.appId}
                          src={game.coverUrl ?? game.headerUrl}
                          title={game.title}
                          kind="cover"
                          className="couch-tile__art"
                        />
                        <span className="couch-tile__title">{game.title}</span>
                        <span className="couch-tile__meta">
                          {game.installed ? "Instalado" : "No instalado"}
                        </span>
                      </button>
                    </ContextMenuTrigger>
                    <ContextMenuContent aria-label={`Acciones rápidas de ${game.title}`}>
                      <ContextMenuLabel>{game.title}</ContextMenuLabel>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        onSelect={() => {
                          setFocusIndex(index);
                          playGame(game);
                        }}
                      >
                        <IconPlayerPlay aria-hidden="true" />
                        {game.installed ? "Jugar" : "Instalar"}
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() => {
                          setFocusIndex(index);
                          openStoreFor(game);
                        }}
                      >
                        <IconBrandSteam aria-hidden="true" /> Abrir en la tienda oficial
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        onSelect={() => {
                          navigator.clipboard?.writeText(game.title).catch(() => undefined);
                        }}
                      >
                        <IconCopy aria-hidden="true" /> Copiar título
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </li>
              ))}
            </ul>

            <aside className="couch__detail" aria-label="Ficha del juego enfocado">
              {focused ? (
                <>
                  <Artwork
                    appId={focused.appId}
                    src={focused.headerUrl ?? focused.coverUrl}
                    title={focused.title}
                    kind="header"
                    className="couch__detail-art"
                  />
                  <h2>{focused.title}</h2>
                  <p className="couch__detail-status">
                    <span
                      className="couch__detail-dot"
                      style={{ background: focused.statusColor }}
                      aria-hidden="true"
                    />
                    {focused.statusName}
                    {focused.installed ? " · Instalado" : " · No instalado"}
                  </p>
                  <ProgressMeter
                    value={focused.progress}
                    label={`Progreso de ${focused.title}`}
                    className="couch__detail-progress"
                  />
                  <dl className="couch__detail-facts">
                    <div>
                      <dt>Tiempo jugado</dt>
                      <dd>{formatPlaytime(focused.playtimeMinutes)}</dd>
                    </div>
                    <div>
                      <dt>Última partida</dt>
                      <dd>{formatDate(focused.lastPlayedAt)}</dd>
                    </div>
                    {focused.steamDeckStatus ? (
                      <div>
                        <dt>Steam Deck</dt>
                        <dd>{formatSteamDeckStatus(focused.steamDeckStatus)}</dd>
                      </div>
                    ) : null}
                    {detail?.developer ? (
                      <div>
                        <dt>Estudio</dt>
                        <dd>{detail.developer}</dd>
                      </div>
                    ) : null}
                  </dl>
                  {detail?.shortDescription ? (
                    <p className="couch__detail-summary">{detail.shortDescription}</p>
                  ) : detailQuery.isPending ? (
                    <p className="couch__detail-summary" aria-hidden="true">
                      Cargando la ficha…
                    </p>
                  ) : null}
                  {incompatibleNote ? (
                    <p className="couch__detail-incompatible" role="status">
                      {incompatibleNote}
                    </p>
                  ) : null}
                  <div className="couch__detail-actions">
                    {/* Un juego ya instalado se puede lanzar aunque la tienda no
                        lo publique para este sistema: puede correr por otra vía.
                        Lo que no se ofrece es empezar una instalación que la
                        tienda no va a poder completar. */}
                    {focused.installed || !incompatibleNote ? (
                      <button
                        type="button"
                        className="couch-action couch-action--primary"
                        aria-label={
                          focused.installed
                            ? `Jugar a ${focused.title}`
                            : `Instalar ${focused.title}`
                        }
                        onClick={playFocused}
                      >
                        {focused.installed ? (
                          <IconPlayerPlay aria-hidden="true" size={22} stroke={1.7} />
                        ) : (
                          <IconDownload aria-hidden="true" size={22} stroke={1.7} />
                        )}
                        {focused.installed ? "Jugar" : "Instalar"}
                      </button>
                    ) : null}
                    <button
                      type="button"
                      className="couch-action"
                      aria-label={`Abrir la tienda de ${focused.title}`}
                      onClick={openStore}
                    >
                      <IconBrandSteam aria-hidden="true" size={22} stroke={1.7} />
                      Tienda
                    </button>
                  </div>
                </>
              ) : null}
            </aside>
          </>
        )}
      </div>

      <footer className="couch__hints">
        <ul aria-label="Controles del modo sofá">
          {HINTS.map((hint) => (
            <li key={hint.button}>
              <span className="couch__hint-button" aria-hidden="true">
                {hint.button}
              </span>
              <span className="couch__hint-key" aria-hidden="true">
                {hint.keyboard}
              </span>
              <span className="couch__hint-label">
                <span className="sr-only">{`${hint.button} o ${hint.keyboard}: `}</span>
                {hint.label}
              </span>
            </li>
          ))}
        </ul>
        <p
          className="couch__announcement"
          role={actionFailed ? "alert" : "status"}
          aria-live="polite"
          data-failed={actionFailed ? "true" : undefined}
        >
          {announcement}
        </p>
      </footer>
    </section>
  );
}
