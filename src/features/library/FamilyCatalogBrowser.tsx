import { IconBrandSteam, IconCircleCheck, IconLoader2 } from "@tabler/icons-react";
import { useState } from "react";
import { CatalogBrowser, type CatalogItem } from "@/features/library/CatalogBrowser";
import { formatDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type {
  FamilyCatalogAvailability,
  FamilyCatalogGame,
  FamilyCatalogSort,
  LibraryView,
} from "@/lib/types";

/**
 * Catálogo de Steam Family.
 *
 * Traduce cada juego prestado a una entrada de [`CatalogBrowser`], que es quien
 * pone la rejilla, la virtualización y el comportamiento. Aquí sólo se decide
 * qué dice cada tarjeta, que es lo único propio de este catálogo.
 */
interface Props {
  games: FamilyCatalogGame[];
  total: number;
  view: LibraryView;
  availability: FamilyCatalogAvailability;
  sort: FamilyCatalogSort;
  queryKey: string;
  hasMore: boolean;
  loadingMore: boolean;
  initialScrollOffset?: number | undefined;
  onScrollOffsetChange?: ((offset: number) => void) | undefined;
  onAvailabilityChange: (availability: FamilyCatalogAvailability) => void;
  onSortChange: (sort: FamilyCatalogSort) => void;
  onViewChange: (view: LibraryView) => void;
  onLoadMore: () => void;
  onOpenConfirmed: (appId: number) => void;
}

export function FamilyCatalogBrowser(props: Props) {
  const [feedback, setFeedback] = useState<string>();
  const [openingAppId, setOpeningAppId] = useState<number>();

  const openStore = (game: FamilyCatalogGame) => {
    setOpeningAppId(game.appId);
    setFeedback(undefined);
    void api
      .openStore(game.appId)
      .catch((cause) => setFeedback(`${game.title}: ${getErrorMessage(cause)}`))
      .finally(() => setOpeningAppId(undefined));
  };

  const items: CatalogItem[] = props.games.map((game) => {
    const comprobado = game.availability === "confirmed";
    return {
      key: String(game.appId),
      appId: game.appId,
      title: game.title,
      coverUrl: game.coverUrl ?? undefined,
      iconUrl: game.iconUrl ?? undefined,
      // Sólo se marca la excepción. Rotular las mil ochocientas tarjetas con su
      // estado normal no informa de nada y tapa la portada.
      badge: comprobado
        ? { label: "COMPROBADO", hint: "Vindexa ha visto este juego en tu equipo" }
        : undefined,
      meta: `Compartido · ${formatDate(game.updatedAt)}`,
      columns: [
        comprobado ? "Comprobado" : "Sin comprobar",
        formatDate(props.view === "compact" ? game.updatedAt : game.discoveredAt),
      ],
      // Sin evidencia local no se ofrece la ficha personal: darla por hecha
      // diría que el juego se puede jugar, y eso lo decide Steam al abrirlo.
      onOpen: comprobado ? () => props.onOpenConfirmed(game.appId) : () => openStore(game),
      corner: {
        label: `Abrir ${game.title} en la tienda integrada`,
        busy: openingAppId === game.appId,
        icon:
          openingAppId === game.appId ? (
            <IconLoader2 className="is-spinning" />
          ) : (
            <IconBrandSteam aria-hidden="true" />
          ),
        onClick: () => openStore(game),
      },
    };
  });

  return (
    <CatalogBrowser
      items={items}
      view={props.view}
      resetKey={JSON.stringify([props.queryKey, props.availability, props.sort, props.view])}
      hasMore={props.hasMore}
      loadingMore={props.loadingMore}
      onLoadMore={props.onLoadMore}
      initialScrollOffset={props.initialScrollOffset}
      onScrollOffsetChange={props.onScrollOffsetChange}
      listHeaders={[
        "JUEGO",
        "DISPONIBILIDAD",
        props.view === "compact" ? "ACTUALIZADO" : "DESCUBIERTO",
      ]}
      surface="family-catalog-browser"
      feedback={feedback}
    />
  );
}

/** Icono del distintivo de comprobado, para quien lo quiera reutilizar. */
export const FamilyConfirmedIcon = IconCircleCheck;
