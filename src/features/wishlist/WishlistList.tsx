import { IconSearch, IconTag } from "@tabler/icons-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useMemo, useRef, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { GamePreviewCard } from "@/components/common/GamePreviewCard";
import { SegmentedControl } from "@/components/motion/SegmentedControl";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatCents, observedLabel, WISHLIST_BUCKETS } from "@/features/wishlist/wishlist-model";
import type {
  WishlistBucketId,
  WishlistEntry,
  WishlistOverview,
  WishlistPriceStatus,
} from "@/lib/types";

/**
 * La lista de deseados, densa y recorrible.
 *
 * # Por qué existe además del tablero
 *
 * El tablero por intención funciona con veinte juegos. Con mil cuatrocientos —lo
 * que trae una importación de Steam— tres carriles quedan vacíos ocupando tres
 * cuartos del ancho y el cuarto se convierte en un desplazamiento sin fondo, con
 * una tarjeta de cinco líneas por juego que repite cuatro veces que no se sabe
 * el precio.
 *
 * Aquí cada juego ocupa **una fila**, se pintan sólo las que caben en pantalla y
 * se puede buscar, filtrar por carril y ordenar por lo que de verdad se mira:
 * el descuento.
 *
 * # Qué se calla
 *
 * Un precio que no se ha podido consultar no escribe nada. «Sin precio objetivo
 * ni precio consultado» era decir dos veces que no se sabe nada, y ocupaba más
 * que el propio título.
 */

type Orden = "descuento" | "precio" | "titulo" | "reciente";

const ORDENES: { value: Orden; label: string }[] = [
  { value: "descuento", label: "Mayor descuento" },
  { value: "precio", label: "Precio más bajo" },
  { value: "titulo", label: "Título" },
  { value: "reciente", label: "Añadido hace menos" },
];

/** Alto de una fila. Fijo: el virtualizador necesita saberlo antes de pintar. */
const ROW_HEIGHT = 34;

export function WishlistList({
  overview,
  prices,
  selectedAppId,
  onSelect,
  renderRowMenu,
}: {
  overview: WishlistOverview | undefined;
  prices: readonly WishlistPriceStatus[];
  selectedAppId?: number | undefined;
  onSelect: (appId: number) => void;
  /** Menú contextual de cada fila, que aporta la pantalla dueña de las acciones. */
  renderRowMenu?:
    | ((entry: WishlistEntry, children: React.ReactNode) => React.ReactNode)
    | undefined;
}) {
  const [bucket, setBucket] = useState<WishlistBucketId | "todos">("todos");
  const [orden, setOrden] = useState<Orden>("descuento");
  const [busqueda, setBusqueda] = useState("");
  const scroller = useRef<HTMLDivElement>(null);

  const precioDe = useMemo(() => {
    const index = new Map<number, WishlistPriceStatus>();
    for (const status of prices) index.set(status.appId, status);
    return index;
  }, [prices]);

  const todas = useMemo(() => (overview?.buckets ?? []).flatMap((lane) => lane.items), [overview]);

  const filas = useMemo(() => {
    const termino = busqueda.trim().toLowerCase();
    const filtradas = todas.filter((entry) => {
      if (bucket !== "todos" && entry.bucket !== bucket) return false;
      if (termino && !entry.game.title.toLowerCase().includes(termino)) return false;
      return true;
    });
    const precio = (entry: WishlistEntry) => precioDe.get(entry.game.appId)?.price;
    return filtradas.sort((left, right) => {
      switch (orden) {
        case "descuento": {
          // Sin descuento conocido se va al final: ordenar por algo que no se
          // sabe es inventarse un orden.
          const a = precio(left)?.discountPercent ?? -1;
          const b = precio(right)?.discountPercent ?? -1;
          return b - a || left.game.title.localeCompare(right.game.title, "es");
        }
        case "precio": {
          const a = precio(left)?.finalCents ?? Number.POSITIVE_INFINITY;
          const b = precio(right)?.finalCents ?? Number.POSITIVE_INFINITY;
          return a - b || left.game.title.localeCompare(right.game.title, "es");
        }
        case "reciente":
          return right.addedAt.localeCompare(left.addedAt);
        default:
          return left.game.title.localeCompare(right.game.title, "es");
      }
    });
  }, [bucket, busqueda, orden, precioDe, todas]);

  const virtualizer = useVirtualizer({
    count: filas.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  const enOferta = filas.filter(
    (entry) => (precioDe.get(entry.game.appId)?.price?.discountPercent ?? 0) > 0,
  ).length;

  return (
    <section className="wishlist-list" aria-label="Lista de deseados">
      <div className="wishlist-list__toolbar">
        <SegmentedControl
          label="Carril"
          value={bucket}
          onValueChange={(value) => setBucket(value as WishlistBucketId | "todos")}
          options={[
            { value: "todos", label: `Todos · ${todas.length}` },
            ...WISHLIST_BUCKETS.map((option) => ({
              value: option.id,
              label: `${option.label} · ${todas.filter((e) => e.bucket === option.id).length}`,
            })),
          ]}
        />
        <div className="wishlist-list__search">
          <IconSearch aria-hidden="true" size={14} />
          <Input
            aria-label="Buscar en deseados"
            placeholder="Buscar por título"
            value={busqueda}
            onChange={(event) => setBusqueda(event.currentTarget.value)}
          />
        </div>
        <Select value={orden} onValueChange={(value) => setOrden(value as Orden)}>
          <SelectTrigger aria-label="Ordenar por" className="wishlist-list__sort">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {ORDENES.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* El recuento dice lo que hay de verdad y lo que se está enseñando: una
          cifra que no coincide con la lista a la que acompaña es peor que
          ninguna. */}
      <p className="wishlist-list__count" role="status">
        {filas.length === todas.length
          ? `${todas.length.toLocaleString("es-ES")} juegos`
          : `${filas.length.toLocaleString("es-ES")} de ${todas.length.toLocaleString("es-ES")}`}
        {enOferta > 0 && (
          <span className="wishlist-list__deals">
            <IconTag aria-hidden="true" size={12} />
            {enOferta === 1 ? "1 en oferta" : `${enOferta} en oferta`}
          </span>
        )}
      </p>

      {filas.length === 0 ? (
        <p className="wishlist-list__empty">
          {todas.length === 0
            ? "Todavía no hay nada en deseados."
            : "Ningún juego de este carril coincide con la búsqueda."}
        </p>
      ) : (
        <div className="wishlist-list__scroller" ref={scroller}>
          <div
            className="wishlist-list__canvas"
            style={{ height: `${virtualizer.getTotalSize()}px` }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const entry = filas[item.index];
              if (!entry) return null;
              const fila = (
                <WishlistRow
                  entry={entry}
                  status={precioDe.get(entry.game.appId)}
                  selected={entry.game.appId === selectedAppId}
                  offset={item.start}
                  onSelect={() => onSelect(entry.game.appId)}
                />
              );
              return (
                <div key={entry.game.appId}>
                  {renderRowMenu ? renderRowMenu(entry, fila) : fila}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
}

function WishlistRow({
  entry,
  status,
  selected,
  offset,
  onSelect,
}: {
  entry: WishlistEntry;
  status: WishlistPriceStatus | undefined;
  selected: boolean;
  offset: number;
  onSelect: () => void;
}) {
  const price = status?.price;
  const descuento = price?.discountPercent ?? 0;
  return (
    <GamePreviewCard
      appId={entry.game.appId}
      title={entry.game.title}
      fallback={
        <Artwork
          appId={entry.game.appId}
          src={entry.game.coverUrl}
          title={entry.game.title}
          kind="cover"
        />
      }
      {...(price
        ? {
            headline: (
              <>
                <b>{formatCents(price.finalCents, price.currency)}</b>
                {descuento > 0 && (
                  <>
                    <s>{formatCents(price.initialCents, price.currency)}</s>
                    <span>−{descuento} %</span>
                  </>
                )}
              </>
            ),
          }
        : {})}
      facts={facts(entry, status)}
    >
      <button
        type="button"
        className="wishlist-list__row"
        data-selected={selected}
        data-meets-target={status?.meetsTarget === true}
        style={{ transform: `translateY(${offset}px)` }}
        onClick={onSelect}
      >
        {descuento > 0 ? (
          <span className="wishlist-list__discount">−{descuento} %</span>
        ) : (
          <span className="wishlist-list__discount" data-empty="true" aria-hidden="true" />
        )}
        <span className="wishlist-list__cover" aria-hidden="true">
          <Artwork
            appId={entry.game.appId}
            src={entry.game.coverUrl}
            title={entry.game.title}
            kind="cover"
          />
        </span>
        <span className="wishlist-list__title">{entry.game.title}</span>
        {entry.game.inLibrary && <span className="wishlist-list__owned">Ya lo tienes</span>}
        {/* Los importes van juntos en una celda: con el precio y el precio
            anterior sueltos, una fila con «Ya lo tienes» tenía un elemento más
            que columnas y se salía de su alto solapándose con la de al lado. */}
        <span className="wishlist-list__prices">
          {price ? (
            <>
              {descuento > 0 && (
                <s className="wishlist-list__before">
                  {formatCents(price.initialCents, price.currency)}
                </s>
              )}
              <span className="wishlist-list__amount">
                {formatCents(price.finalCents, price.currency)}
              </span>
            </>
          ) : (
            // Un precio que no se ha podido consultar no escribe cuatro líneas
            // diciendo que no se sabe: escribe una palabra.
            <span className="wishlist-list__amount" data-unknown="true">
              sin precio
            </span>
          )}
        </span>
      </button>
    </GamePreviewCard>
  );
}

/** Sólo lo que se sabe. Un hueco no se rellena con un cero ni con una frase. */
function facts(
  entry: WishlistEntry,
  status: WishlistPriceStatus | undefined,
): { label: string; value: string }[] {
  const salida: { label: string; value: string }[] = [];
  const bucket = WISHLIST_BUCKETS.find((option) => option.id === entry.bucket);
  if (bucket) salida.push({ label: "Carril", value: bucket.label });
  if (status?.targetCents != null && status.targetCurrency) {
    salida.push({
      label: "Objetivo",
      value: formatCents(status.targetCents, status.targetCurrency),
    });
  }
  if (status?.price) {
    // La vigencia se dice con las palabras que ya usa el resto de la pantalla:
    // «consultado hace…», no una fecha cruda.
    salida.push({
      label: "Visto",
      value: observedLabel(status.price.observedAt, status.price.freshness),
    });
  }
  if (entry.note.trim()) salida.push({ label: "Nota", value: entry.note.trim() });
  return salida;
}
