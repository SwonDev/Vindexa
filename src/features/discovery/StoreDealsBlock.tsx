import {
  IconExternalLink,
  IconHeartPlus,
  IconLoader2,
  IconRefresh,
  IconTag,
  IconX,
} from "@tabler/icons-react";
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
import { formatRelativeDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { DealCandidate } from "@/lib/types";

/**
 * Rebajas que todavía no son tuyas, ordenadas por lo que te gusta.
 *
 * # Dos tiendas
 *
 * Steam y GOG. Cada fila dice de cuál es, porque el precio de una no se paga en
 * la otra. Las de GOG no tienen AppID de Steam: no se puede enseñar sus
 * capturas ni pasarlas a deseados —que se llevan por AppID—, y eso se dice en
 * el menú en vez de ofrecer un botón que no haría nada.
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

/**
 * A partir de cuánto una lista de rebajas deja de ser fiable.
 *
 * La tanda va cada seis horas mientras la aplicación está abierta. Si estuvo
 * cerrada un fin de semana, lo guardado puede llevar días caducado: una rebaja
 * terminada lleva a la tienda a pagar el precio completo creyendo que hay
 * descuento, así que pasado el doble del ritmo normal se avisa.
 */
const STALE_HOURS = 12;

function isStale(checkedAt: string): boolean {
  const cuando = Date.parse(checkedAt);
  if (Number.isNaN(cuando)) return true;
  return Date.now() - cuando >= STALE_HOURS * 60 * 60 * 1000;
}

export function StoreDealsBlock() {
  const queryClient = useQueryClient();
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState<string>();
  const [report, setReport] = useState<string>();

  const deals = useQuery({
    queryKey: ["store-deals"],
    queryFn: () => api.storeDeals(40),
    staleTime: 15 * 60 * 1000,
    retry: false,
  });

  const dismiss = useMutation({
    mutationFn: (deal: DealCandidate) => api.dismissStoreDeal(deal.store, deal.externalId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["store-deals"] }),
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const openStore = useMutation({
    // La dirección la resuelve el backend con la pareja tienda-identificador:
    // así una oferta de GOG abre GOG y una de Steam abre Steam, sin que la
    // interfaz componga direcciones.
    mutationFn: (deal: DealCandidate) => api.openStoreDeal(deal.store, deal.externalId),
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const wish = useMutation({
    mutationFn: (deal: DealCandidate) => {
      // Los deseados se llevan por AppID de Steam. Sin él no hay nada que
      // guardar, y adivinarlo por el título sería inventar una equivalencia.
      if (deal.appId == null) {
        throw new Error("Los deseados se llevan por AppID de Steam y esta oferta no tiene.");
      }
      return api.saveWishlistEntry({
        appId: deal.appId,
        bucket: "waiting_sale",
        priority: 0,
        note: "",
      });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["store-deals"] });
      void queryClient.invalidateQueries({ queryKey: ["wishlist-overview"] });
    },
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  const refresh = useMutation({
    mutationFn: () => api.refreshStoreDeals(),
    onSuccess: (report) => {
      void queryClient.invalidateQueries({ queryKey: ["store-deals"] });
      // Lo que faltó se dice: una tienda caída deja una lista corta con pinta
      // de completa, y eso es peor que no traer nada.
      const partes = [
        `${report.received} rebajas leídas`,
        `${report.discovered} nuevas`,
        `${report.alreadyKnown} ya tuyas o en deseados`,
      ];
      if (report.unavailable.length > 0) {
        partes.push(`sin respuesta: ${report.unavailable.map(storeName).join(", ")}`);
      }
      setError(undefined);
      setReport(`${partes.join(" · ")}.`);
    },
    onError: (cause) => {
      setReport(undefined);
      setError(getErrorMessage(cause));
    },
  });

  const items = deals.data?.deals ?? [];
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
        {/* Las tiendas se repasan solas cada seis horas. El botón está para
            cuando no se quiere esperar: una rebaja que se acaba no espera. */}
        <Button
          variant="ghost"
          size="xs"
          disabled={refresh.isPending}
          onClick={() => refresh.mutate()}
        >
          {refresh.isPending ? (
            <IconLoader2 className="is-spinning" aria-hidden="true" size={13} />
          ) : (
            <IconRefresh aria-hidden="true" size={13} />
          )}
          {refresh.isPending ? "Preguntando…" : "Actualizar"}
        </Button>
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
              key={`${deal.store}:${deal.externalId}`}
              deal={deal}
              busy={dismiss.isPending || openStore.isPending || wish.isPending}
              onOpen={() => openStore.mutate(deal)}
              onWish={() => wish.mutate(deal)}
              onDismiss={() => dismiss.mutate(deal)}
            />
          ))}
        </ul>
      )}

      {/* Se despliega y se recoge: dejar sólo la ida obliga a cambiar de
          pestaña para volver a una lista corta. */}
      {(ocultas > 0 || expanded) && (
        <Button variant="ghost" size="xs" onClick={() => setExpanded((abierto) => !abierto)}>
          {expanded ? `Ver sólo ${VISIBLE}` : `Ver las ${items.length}`}
        </Button>
      )}

      {/* Cuándo se miró. Sin esta línea la lista miente por omisión: una rebaja
          que terminó ayer sigue guardada hasta la siguiente tanda. */}
      {!report && !error && deals.data?.checkedAt && (
        <p className="store-deals__note" data-stale={isStale(deals.data.checkedAt)}>
          Consultado {formatRelativeDate(deals.data.checkedAt)}.{" "}
          {isStale(deals.data.checkedAt)
            ? "Algunas pueden haber terminado: pulsa «Actualizar»."
            : "Las tiendas se repasan solas cada seis horas."}
        </p>
      )}

      {report && !error && (
        <p className="store-deals__note" role="status">
          {report}
        </p>
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
            appId={deal.appId ?? null}
            title={deal.title}
            fallback={
              <Artwork
                appId={deal.appId ?? undefined}
                src={deal.headerUrl ?? undefined}
                title={deal.title}
                kind="header"
              />
            }
            headline={
              <>
                <b>{formatAmount(deal)}</b>
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
              {/* De qué tienda es: el mismo juego cuesta cosas distintas en
                  cada una, y sin decirlo el precio no significa nada. */}
              <span className="store-deals__store" data-store={deal.store}>
                {storeName(deal.store)}
              </span>
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
              <span className="store-deals__amount" data-free={deal.finalCents === 0}>
                {formatAmount(deal)}
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
        <ContextMenuItem disabled={busy || deal.appId == null} onSelect={onWish}>
          <IconHeartPlus aria-hidden="true" /> Añadir a deseados
        </ContextMenuItem>
        {deal.appId == null && (
          <ContextMenuLabel className="store-deals__hint">
            Los deseados se llevan por AppID de Steam, y esta oferta es de {storeName(deal.store)}.
          </ContextMenuLabel>
        )}
        <ContextMenuSeparator />
        <ContextMenuItem variant="destructive" disabled={busy} onSelect={onDismiss}>
          <IconX aria-hidden="true" /> No me interesa
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/**
 * Un precio de cero no se escribe «0,00 €».
 *
 * Steam y GOG regalan juegos durante unos días modelándolo como un descuento
 * del cien por cien. «0,00 €» obliga a traducirlo mentalmente; «Gratis» dice lo
 * que es, que además es lo más accionable que puede haber en una lista de
 * rebajas —por eso van primero.
 */
function formatAmount(deal: DealCandidate): string {
  return deal.finalCents === 0 ? "Gratis" : formatCents(deal.finalCents, deal.currency);
}

/** Cómo se llama cada tienda por escrito. */
function storeName(store: string): string {
  if (store === "gog") return "GOG";
  if (store === "steam") return "Steam";
  return store;
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
  salida.push({ label: "Tienda", value: storeName(deal.store) });
  return salida;
}

function formatCents(cents: number, currency: string): string {
  return new Intl.NumberFormat("es-ES", { style: "currency", currency }).format(cents / 100);
}
