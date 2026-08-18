import { IconExternalLink, IconLoader2, IconPlayerPlay } from "@tabler/icons-react";
import { useState } from "react";
import { CatalogBrowser, type CatalogItem } from "@/features/library/CatalogBrowser";
import { formatBytes } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { ExternalGame, ExternalStoreId, LibraryView } from "@/lib/types";

/**
 * Catálogo de una tienda vinculada: Epic, GOG o itch.io.
 *
 * Traduce cada juego a una entrada de [`CatalogBrowser`], que es quien pone la
 * rejilla y el comportamiento. Antes estos juegos vivían en una lista de texto
 * dentro de Ajustes, sin portadas y sin nada en común con la biblioteca.
 *
 * Un juego de otra tienda **no** tiene AppID de Steam, así que su portada se
 * pinta desde la URL que publica la propia tienda. Cuando Vindexa lo ha
 * emparejado con un juego de tu biblioteca de Steam, se dice en su línea: es lo
 * que explica por qué el mismo juego aparece en dos sitios.
 */
interface Props {
  store: ExternalStoreId;
  storeLabel: string;
  games: ExternalGame[];
  view: LibraryView;
  queryKey: string;
  hasMore: boolean;
  loadingMore: boolean;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  onLoadMore: () => void;
  onOpenMatched: (appId: number) => void;
}

export function StoreCatalogBrowser(props: Props) {
  const [feedback, setFeedback] = useState<string>();
  const [launching, setLaunching] = useState<string>();

  const launch = (game: ExternalGame) => {
    setLaunching(game.externalId);
    setFeedback(undefined);
    void api
      .launchExternalGame(game.store, game.externalId)
      .catch((cause) => setFeedback(`${game.title}: ${getErrorMessage(cause)}`))
      .finally(() => setLaunching(undefined));
  };

  const items: CatalogItem[] = props.games.map((game) => {
    const emparejado = game.matchedAppId !== null;
    const partes = [props.storeLabel];
    if (game.installed) partes.push("Instalado");
    if (game.sizeOnDisk) partes.push(formatBytes(game.sizeOnDisk));
    if (emparejado) partes.push("También en Steam");
    return {
      key: `${game.store}:${game.externalId}`,
      // Sin AppID propio: la portada viene de la tienda, no de la CDN de Steam.
      // Con emparejamiento sí lo hay, y entonces la caché local lo aprovecha.
      appId: game.matchedAppId ?? undefined,
      title: game.title,
      coverUrl: game.coverUrl ?? game.headerUrl ?? undefined,
      iconUrl: game.headerUrl ?? game.coverUrl ?? undefined,
      badge: game.installed
        ? { label: "INSTALADO", hint: `Instalado desde ${props.storeLabel}` }
        : undefined,
      meta: partes.join(" · "),
      columns: [
        game.installed ? "Instalado" : "No instalado",
        emparejado ? (game.matchedTitle ?? "Emparejado") : "Sin emparejar",
      ],
      // Sólo hay ficha propia si el juego está emparejado con uno de Steam. Sin
      // eso no hay nada que abrir: Vindexa no guarda ficha de un juego de otra
      // tienda, y fingir una sería inventarla.
      onOpen:
        game.matchedAppId !== null
          ? () => props.onOpenMatched(game.matchedAppId as number)
          : undefined,
      corner: game.installed
        ? {
            label: `Jugar a ${game.title}`,
            busy: launching === game.externalId,
            icon:
              launching === game.externalId ? (
                <IconLoader2 className="is-spinning" />
              ) : (
                <IconPlayerPlay aria-hidden="true" />
              ),
            onClick: () => launch(game),
          }
        : {
            label: `${game.title} no está instalado`,
            icon: <IconExternalLink aria-hidden="true" />,
            busy: true,
            onClick: () => undefined,
          },
    };
  });

  return (
    <CatalogBrowser
      items={items}
      view={props.view}
      resetKey={JSON.stringify([props.store, props.queryKey, props.view])}
      hasMore={props.hasMore}
      loadingMore={props.loadingMore}
      onLoadMore={props.onLoadMore}
      initialScrollOffset={props.initialScrollOffset}
      onScrollOffsetChange={props.onScrollOffsetChange}
      listHeaders={["JUEGO", "ESTADO", "EN TU BIBLIOTECA"]}
      surface="store-catalog-browser"
      feedback={feedback}
    />
  );
}
