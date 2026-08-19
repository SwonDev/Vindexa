import { IconExternalLink, IconGift, IconLoader2, IconX } from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { api, getErrorMessage } from "@/lib/tauri";
import type { EpicFreeOffer } from "@/lib/types";

/**
 * Lo que Epic regala esta semana.
 *
 * # Por qué está en Vindexa y no en la tienda
 *
 * Epic regala un juego cada jueves y la promoción caduca. La tienda te lo dice
 * si entras; Vindexa te lo dice sin entrar, y añade lo único que la tienda no
 * sabe: **si ya lo tienes**. Reclamarlo es un clic que abre la ficha en el
 * navegador integrado, donde ya estás identificado.
 *
 * # Lo que no hace
 *
 * No reclama por ti. Conducir una sesión ajena por un flujo de compra es otra
 * cosa distinta de avisar, y no es lo que esta aplicación hace.
 *
 * # Qué se enseña y qué no
 *
 * Lo vigente primero; lo anunciado, después y apagado. Lo descartado a mano
 * desaparece. Sin fecha de fin no se dice «quedan horas»: se calla, porque
 * inventar una cuenta atrás es peor que no darla.
 */
export function EpicFreeBlock() {
  const queryClient = useQueryClient();
  const [error, setError] = useState<string>();

  const offers = useQuery({
    queryKey: ["epic-free"],
    // Al abrir se pregunta a Epic: la respuesta pesa treinta kilobytes y la
    // promoción cambia cada jueves, así que enseñar lo de la semana pasada
    // sería enseñar algo caducado.
    queryFn: () => api.epicFreeGames(true),
    staleTime: 30 * 60 * 1000,
    retry: false,
  });

  const claim = useMutation({
    mutationFn: (offer: EpicFreeOffer) => api.openEpicFreeGame(offer.storeUrl),
    onError: (cause) => setError(getErrorMessage(cause)),
  });
  const dismiss = useMutation({
    mutationFn: (offer: EpicFreeOffer) => api.dismissEpicFreeGame(offer.offerId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["epic-free"] }),
    onError: (cause) => setError(getErrorMessage(cause)),
  });

  const visibles = (offers.data ?? []).filter((offer) => !offer.dismissed);
  const vigentes = visibles.filter((offer) => offer.state === "current");
  const anunciados = visibles.filter((offer) => offer.state === "upcoming");

  // Sin nada que enseñar el bloque no ocupa sitio. Un recuadro que dice «no hay
  // nada» en una columna ya larga es ruido, no información.
  if (!offers.isPending && visibles.length === 0 && !offers.isError) {
    return null;
  }

  return (
    <section className="epic-free" aria-labelledby="epic-free-heading">
      <header className="epic-free__head">
        <IconGift aria-hidden="true" size={15} />
        <h2 id="epic-free-heading">Gratis en Epic</h2>
        {vigentes.length > 0 && (
          <span className="epic-free__count">
            {vigentes.length === 1 ? "1 esta semana" : `${vigentes.length} esta semana`}
          </span>
        )}
      </header>

      {offers.isPending ? (
        <p className="epic-free__note" role="status">
          <IconLoader2 className="is-spinning" aria-hidden="true" size={13} /> Preguntando a Epic…
        </p>
      ) : offers.isError ? (
        <p className="epic-free__note" role="alert">
          No se pudo preguntar a Epic. {getErrorMessage(offers.error)}
        </p>
      ) : (
        <ul className="epic-free__list">
          {[...vigentes, ...anunciados].map((offer) => (
            <EpicFreeRow
              key={offer.offerId}
              offer={offer}
              busy={claim.isPending || dismiss.isPending}
              onClaim={() => claim.mutate(offer)}
              onDismiss={() => dismiss.mutate(offer)}
            />
          ))}
        </ul>
      )}

      {error && (
        <p className="epic-free__note" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function EpicFreeRow({
  offer,
  busy,
  onClaim,
  onDismiss,
}: {
  offer: EpicFreeOffer;
  busy: boolean;
  onClaim: () => void;
  onDismiss: () => void;
}) {
  const vigente = offer.state === "current";
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <li className="epic-free__item" data-state={offer.state} data-owned={offer.owned}>
          <div className="epic-free__body">
            <p className="epic-free__title">{offer.title}</p>
            <p className="epic-free__meta">
              {offer.owned ? (
                <span className="epic-free__owned">Ya lo tienes</span>
              ) : (
                describeWindow(offer)
              )}
              {offer.originalPriceCents != null && offer.currency && (
                <span className="epic-free__was">
                  {formatAmount(offer.originalPriceCents, offer.currency)}
                </span>
              )}
            </p>
          </div>
          {/* Quien ya lo tiene no necesita el botón; el menú contextual sigue
              ofreciendo abrir la ficha por si quiere mirarla. */}
          {vigente && !offer.owned && (
            <Button size="xs" variant="outline" disabled={busy} onClick={onClaim}>
              <IconExternalLink aria-hidden="true" /> Reclamar
            </Button>
          )}
        </li>
      </ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${offer.title}`}>
        <ContextMenuLabel>{offer.title}</ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuItem disabled={busy} onSelect={onClaim}>
          <IconExternalLink aria-hidden="true" />
          {vigente && !offer.owned ? "Reclamar en Epic" : "Abrir la ficha en Epic"}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem variant="destructive" disabled={busy} onSelect={onDismiss}>
          <IconX aria-hidden="true" /> No me interesa
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/**
 * Cuánto queda, dicho con la precisión que se tiene.
 *
 * Sin fecha de fin no se inventa una cuenta atrás; con ella se redondea a días
 * u horas, que es como se piensa en «me da tiempo».
 */
function describeWindow(offer: EpicFreeOffer): string {
  if (offer.state === "upcoming") {
    return offer.startsAt ? `Desde el ${formatDay(offer.startsAt)}` : "Anunciado";
  }
  if (offer.hoursLeft == null) {
    return "Gratis ahora";
  }
  if (offer.hoursLeft >= 48) {
    return `Quedan ${Math.floor(offer.hoursLeft / 24)} días`;
  }
  if (offer.hoursLeft >= 1) {
    return `Quedan ${offer.hoursLeft} h`;
  }
  return "Acaba en menos de una hora";
}

function formatDay(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "una fecha sin confirmar";
  return date.toLocaleDateString("es-ES", { day: "numeric", month: "short" });
}

function formatAmount(cents: number, currency: string): string {
  return new Intl.NumberFormat("es-ES", { style: "currency", currency }).format(cents / 100);
}
