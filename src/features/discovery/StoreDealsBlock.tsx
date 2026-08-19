import { IconExternalLink, IconHeartPlus, IconLoader2, IconTag, IconX } from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { GamePreviewCard } from "@/components/common/GamePreviewCard";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { describeMatch } from "@/features/discovery/UpcomingReleasesBlock";
import { api, getErrorMessage } from "@/lib/tauri";
import type { DealCandidate } from "@/lib/types";

/**
 * Rebajas que todavía no son tuyas, ordenadas por lo que te gusta.
 *
 * # Por qué no es un escaparate
 *
 * La tienda ya sabe enseñar rebajas. Lo que no sabe es cuáles te interesan, y
 * eso es lo único que justifica traerlas aquí: cada una viene puntuada contra
 * el mismo modelo de gustos que ordena los próximos lanzamientos, el que se
 * calcula con tu historial y no sale de tu ordenador.
 *
 * Lo que ya tienes y lo que ya deseas no aparece: lo primero no es una oferta y
 * lo segundo tiene su propia sección con tu precio objetivo.
 *
 * # Lo que no se sabe se dice
 *
 * Una oferta sin puntuar —aún no se han podido pedir sus géneros— se enseña
 * detrás y sin nota, no con un cero. Cero significaría «no te interesa», y eso
 * no se sabe.
 */

/** Cuántas se enseñan sin desplegar. Bastantes para elegir, pocas para leerlas. */
const VISIBLE = 6;

export function StoreDealsBlock() {
  const queryClient = useQueryClient();
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState<string>();

  const deals = useQuery({
    queryKey: ["store-deals"],
    queryFn: () => api.storeDeals(40),
    staleTime: 15 * 60 * 1000,
    retry: false,
  });

  const dismiss = useMutation({
    mutationFn: (deal: DealCandidate) => api.dismissStoreDeal(deal.appId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["store-deals"] }),
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const openStore = useMutation({
    mutationFn: (deal: DealCandidate) => api.openStore(deal.appId),
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const wish = useMutation({
    mutationFn: (deal: DealCandidate) =>
      api.saveWishlistEntry({
        appId: deal.appId,
        bucket: "waiting_sale",
        priority: 0,
        note: "",
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["store-deals"] });
      void queryClient.invalidateQueries({ queryKey: ["wishlist-overview"] });
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  const items = deals.data ?? [];
  // Sin ofertas el bloque no ocupa sitio: un recuadro vacío en una columna larga
  // es ruido, no información.
  if (!deals.isPending && items.length === 0 && !deals.isError) return null;

  const visibles = expanded ? items : items.slice(0, VISIBLE);
  const ocultas = items.length - visibles.length;
  // La afinidad viaja de 0 a 1, igual que en los lanzamientos. Tratarla como
  // porcentaje ya escrito daba «1 %» donde había una coincidencia del 56 %.
  const interesantes = items.filter(
    (deal) => describeMatch(deal.matchScore ?? 0).level === "high",
  ).length;

  return (
    <section className="store-deals" aria-labelledby="store-deals-heading">
      <header className="store-deals__head">
        <IconTag aria-hidden="true" size={15} />
        <h2 id="store-deals-heading">Ofertas para ti</h2>
        {interesantes > 0 && (
          <span className="store-deals__count">
            {interesantes === 1 ? "1 encaja contigo" : `${interesantes} encajan contigo`}
          </span>
        )}
      </header>

      {deals.isPending ? (
        <p className="store-deals__note" role="status">
          <IconLoader2 className="is-spinning" aria-hidden="true" size={13} /> Mirando las rebajas…
        </p>
      ) : deals.isError ? (
        <p className="store-deals__note" role="alert">
          No se pudieron leer las ofertas. {getErrorMessage(deals.error)}
        </p>
      ) : (
        <ul className="store-deals__list">
          {visibles.map((deal) => (
            <DealRow
              key={deal.appId}
              deal={deal}
              busy={dismiss.isPending || openStore.isPending || wish.isPending}
              onOpen={() => openStore.mutate(deal)}
              onWish={() => wish.mutate(deal)}
              onDismiss={() => dismiss.mutate(deal)}
            />
          ))}
        </ul>
      )}

      {ocultas > 0 && (
        <Button variant="ghost" size="xs" onClick={() => setExpanded(true)}>
          Ver las {items.length}
        </Button>
      )}

      {error && (
        <p className="store-deals__note" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function DealRow({
  deal,
  busy,
  onOpen,
  onWish,
  onDismiss,
}: {
  deal: DealCandidate;
  busy: boolean;
  onOpen: () => void;
  onWish: () => void;
  onDismiss: () => void;
}) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <li className="store-deals__item">
          <GamePreviewCard
            side="bottom"
            appId={deal.appId}
            title={deal.title}
            fallback={
              <Artwork
                appId={deal.appId}
                src={deal.headerUrl ?? undefined}
                title={deal.title}
                kind="header"
              />
            }
            headline={
              <>
                <b>{formatCents(deal.finalCents, deal.currency)}</b>
                {deal.discountPercent > 0 && (
                  <>
                    <s>{formatCents(deal.initialCents, deal.currency)}</s>
                    <span>−{deal.discountPercent} %</span>
                  </>
                )}
              </>
            }
            facts={facts(deal)}
          >
            <button type="button" className="store-deals__target" onClick={onOpen} disabled={busy}>
              {deal.discountPercent > 0 ? (
                <span className="store-deals__discount">−{deal.discountPercent} %</span>
              ) : (
                <span className="store-deals__discount" data-empty="true" aria-hidden="true" />
              )}
              <span className="store-deals__title">{deal.title}</span>
              {/* La coincidencia sólo se enseña cuando se ha podido calcular:
                  sin rasgos no hay puntuación, y un cero sería una afirmación. */}
              {deal.matchScore != null && (
                <span
                  className="store-deals__match"
                  data-strong={describeMatch(deal.matchScore).level === "high"}
                  title={describeMatch(deal.matchScore).band}
                >
                  {describeMatch(deal.matchScore).percent} %
                </span>
              )}
              <span className="store-deals__amount">
                {formatCents(deal.finalCents, deal.currency)}
              </span>
            </button>
          </GamePreviewCard>
        </li>
      </ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${deal.title}`}>
        <ContextMenuLabel>{deal.title}</ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuItem disabled={busy} onSelect={onOpen}>
          <IconExternalLink aria-hidden="true" /> Abrir en la tienda oficial
        </ContextMenuItem>
        <ContextMenuItem disabled={busy} onSelect={onWish}>
          <IconHeartPlus aria-hidden="true" /> Añadir a deseados
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem variant="destructive" disabled={busy} onSelect={onDismiss}>
          <IconX aria-hidden="true" /> No me interesa
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** Sólo lo que se sabe. Una razón vacía no ocupa una fila diciendo nada. */
function facts(deal: DealCandidate): { label: string; value: string }[] {
  const salida: { label: string; value: string }[] = [];
  if (deal.matchScore != null) {
    const match = describeMatch(deal.matchScore);
    salida.push({ label: "Coincidencia", value: `${match.band} · ${match.percent} %` });
  }
  if (deal.matchReason.trim()) {
    salida.push({ label: "Por qué", value: deal.matchReason.trim() });
  }
  return salida;
}

function formatCents(cents: number, currency: string): string {
  return new Intl.NumberFormat("es-ES", { style: "currency", currency }).format(cents / 100);
}
