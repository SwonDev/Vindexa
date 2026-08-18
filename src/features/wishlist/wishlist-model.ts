import { formatRelativeDate } from "@/lib/format";
import type {
  CuratedAccent,
  CuratedListKind,
  GameVideo,
  PriceFreshness,
  VideoKind,
  WishlistBucket,
  WishlistBucketId,
  WishlistEntry,
  WishlistOverview,
  WishlistPriceStatus,
} from "@/lib/types";

/**
 * Vocabulario y aritmética de la pantalla de Deseados.
 *
 * Todo lo que hay aquí es puro: no toca el DOM, no llama al backend y no
 * depende de React. Es el sitio donde vive la regla de honestidad del proyecto
 * —un total con entradas sin precio nunca se presenta como cerrado— para que
 * pueda comprobarse con una prueba de unidad en vez de mirando la pantalla.
 */

/* --- Cubos de intención -------------------------------------------------- */

export interface WishlistBucketMeta {
  id: WishlistBucketId;
  label: string;
  /** Qué significa realmente estar en este cubo. */
  hint: string;
  /** Frase del carril vacío: dice qué poner aquí, no que esté vacío. */
  empty: string;
}

export const WISHLIST_BUCKETS = [
  {
    id: "buying_now",
    label: "Comprar ya",
    hint: "Decidido; solo falta el momento.",
    empty: "Nada decidido todavía. Arrastra aquí lo que ya sabes que vas a comprar.",
  },
  {
    id: "waiting_sale",
    label: "Esperando oferta",
    hint: "Lo quieres, pero no a este precio.",
    empty: "Aquí van los juegos que solo esperan un descuento. Anótales un precio objetivo.",
  },
  {
    id: "considering",
    label: "Considerando",
    hint: "Aún no está claro que compense.",
    empty: "El sitio para lo que dudas. Guarda un vídeo debajo y decide con calma.",
  },
  {
    id: "watching",
    label: "Vigilando",
    hint: "De lejos, sin decisión tomada.",
    empty: "Para lo que sigues de reojo: parches, reseñas, un posible cambio de rumbo.",
  },
] as const satisfies readonly WishlistBucketMeta[];

const BUCKET_BY_ID = new Map<WishlistBucketId, WishlistBucketMeta>(
  WISHLIST_BUCKETS.map((bucket) => [bucket.id, bucket] as const),
);

export function bucketMeta(id: WishlistBucketId): WishlistBucketMeta {
  return BUCKET_BY_ID.get(id) ?? WISHLIST_BUCKETS[3];
}

export function isWishlistBucketId(value: string): value is WishlistBucketId {
  return BUCKET_BY_ID.has(value as WishlistBucketId);
}

/**
 * El backend puede devolver solo los cubos con contenido. La pantalla necesita
 * los cuatro siempre y en el mismo orden: un carril que aparece y desaparece
 * cambiaría el destino del arrastre a mitad del gesto.
 */
export function normalizeBuckets(overview?: WishlistOverview): WishlistBucket[] {
  const byId = new Map((overview?.buckets ?? []).map((bucket) => [bucket.bucket, bucket]));
  return WISHLIST_BUCKETS.map(({ id }) => {
    const bucket = byId.get(id);
    if (!bucket) return { bucket: id, items: [], total: 0 };
    return { ...bucket, items: [...bucket.items] };
  });
}

/* --- Identificadores de arrastre ----------------------------------------- */

export const wishlistDragId = (appId: number) => `wishlist:${appId}`;
export const wishlistBucketDropId = (bucket: WishlistBucketId) => `wishlist-bucket:${bucket}`;

export function parseWishlistDragId(value: string | number): number | undefined {
  const match = /^wishlist:(\d+)$/.exec(String(value));
  if (!match?.[1]) return undefined;
  const appId = Number(match[1]);
  return Number.isSafeInteger(appId) && appId > 0 ? appId : undefined;
}

export function parseWishlistBucketDropId(value: string | number): WishlistBucketId | undefined {
  const match = /^wishlist-bucket:(.+)$/.exec(String(value));
  if (!match?.[1]) return undefined;
  return isWishlistBucketId(match[1]) ? match[1] : undefined;
}

export function moveWithin<T>(items: readonly T[], from: number, to: number): T[] {
  if (from < 0 || to < 0 || from >= items.length || to >= items.length || from === to) {
    return [...items];
  }
  const next = [...items];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) return [...items];
  next.splice(to, 0, moved);
  return next;
}

/* --- Dinero -------------------------------------------------------------- */

const plainAmount = new Intl.NumberFormat("es-ES", {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

/**
 * Importe en la moneda que anotó la persona. Si el código no es un ISO 4217
 * que el motor reconozca, se escribe la cifra seguida del código tal cual en
 * lugar de inventar un símbolo o descartar el dato.
 */
export function formatCents(cents: number, currency?: string): string {
  const amount = cents / 100;
  const code = (currency ?? "").trim().toUpperCase();
  if (/^[A-Z]{3}$/.test(code)) {
    try {
      return new Intl.NumberFormat("es-ES", { style: "currency", currency: code }).format(amount);
    } catch {
      // Código con forma válida que este motor no conoce: se cae al literal.
    }
  }
  return code ? `${plainAmount.format(amount)} ${code}` : plainAmount.format(amount);
}

/** Céntimos a partir de lo que se escribe en el campo. `null` = texto inválido. */
export function parsePriceInput(raw: string): number | undefined | null {
  const trimmed = raw.trim().replace(",", ".");
  if (!trimmed) return undefined;
  if (!/^\d+(\.\d{0,2})?$/.test(trimmed)) return null;
  const value = Number(trimmed);
  if (!Number.isFinite(value) || value < 0) return null;
  return Math.round(value * 100);
}

/** Céntimos al texto editable del campo, con coma decimal. */
export function priceInputValue(cents?: number): string {
  if (cents === undefined) return "";
  return (cents / 100).toFixed(2).replace(".", ",");
}

export interface WishlistTargetSummary {
  /** Cierto cuando hay entradas sin precio: el total es un suelo, no un cierre. */
  atLeast: boolean;
  entriesWithoutTarget: number;
  currencies: { currency: string; amount: string; entries: number }[];
  /** Cifra o cifras, ya con el «Al menos» delante cuando corresponde. */
  headline: string;
  /** Por qué la cifra es la que es. Vacío solo cuando no hay nada que matizar. */
  caveat: string;
}

/**
 * Agregado de precios objetivo.
 *
 * Dos reglas que no se negocian:
 *
 * 1. **Las monedas no se suman entre sí.** El backend las devuelve separadas a
 *    propósito y aquí se muestran separadas; convertirlas exigiría un tipo de
 *    cambio que Vindexa no tiene y que caducaría al día siguiente.
 * 2. **Con entradas sin precio, el total se presenta como «al menos X».** Un
 *    número redondo bajo una lista incompleta es una mentira cómoda, y esta
 *    pantalla existe justamente para decidir con dinero de por medio.
 */
export function summarizeTargets(overview?: WishlistOverview): WishlistTargetSummary {
  const totals = overview?.targetTotals ?? [];
  const entriesWithoutTarget = overview?.entriesWithoutTarget ?? 0;
  const atLeast = entriesWithoutTarget > 0;
  const currencies = totals.map((total) => ({
    currency: total.currency,
    amount: formatCents(total.totalCents, total.currency),
    entries: total.entries,
  }));

  const caveats: string[] = [];
  if (atLeast) {
    caveats.push(
      entriesWithoutTarget === 1
        ? "1 entrada sin precio objetivo, así que la cifra solo puede ser un mínimo"
        : `${entriesWithoutTarget.toLocaleString("es-ES")} entradas sin precio objetivo, así que la cifra solo puede ser un mínimo`,
    );
  }
  if (currencies.length > 1) {
    caveats.push("monedas distintas: cada una se cuenta por separado, nunca sumadas entre sí");
  }

  if (!currencies.length) {
    return {
      atLeast,
      entriesWithoutTarget,
      currencies,
      headline: "Sin precio objetivo",
      caveat: atLeast
        ? `Ninguna entrada tiene precio anotado: ${entriesWithoutTarget.toLocaleString("es-ES")} pendientes.`
        : "Anota un precio objetivo para ver cuánto costaría la lista.",
    };
  }

  const figures = currencies.map((entry) => entry.amount).join(" · ");
  return {
    atLeast,
    entriesWithoutTarget,
    currencies,
    headline: atLeast ? `Al menos ${figures}` : figures,
    caveat: caveats.length ? `${caveats.join("; ")}.` : "",
  };
}

/* --- Precio observado ---------------------------------------------------- */

/**
 * Lo que la tarjeta enseña sobre el precio de una entrada.
 *
 * `freshness` incluye `"missing"`, que no existe en el backend: allí la
 * ausencia de precio es la ausencia de fila. Aquí hace falta una palabra para
 * pintar ese hueco, porque el hueco **se enseña**: una tarjeta sin precio tiene
 * que decir que no se sabe, no quedarse callada.
 */
export interface PriceLine {
  /** Importe observado, ya formateado. Ausente cuando no se sabe. */
  amount?: string;
  /** Precio de referencia, sólo si es mayor que el vigente. */
  reference?: string;
  /** Descuento publicado, sólo si lo hay. */
  discount?: string;
  /** Mínimo que Vindexa ha visto. Nunca «mínimo histórico» a secas. */
  lowest?: string;
  /** Cuándo se miró, en palabras. */
  observed: string;
  freshness: PriceFreshness | "missing";
  /** Relación con el objetivo, o por qué no la hay. Nunca queda vacío. */
  verdict: string;
  meetsTarget: boolean;
}

/**
 * Cuándo se miró el precio, y si eso todavía significa algo.
 *
 * Un precio observado hace tres semanas no es «el precio»: se enseña con su
 * fecha y con la advertencia delante, en el mismo texto, para que no dependa de
 * un color ni de un `hover`.
 */
export function observedLabel(observedAt: string, freshness: PriceFreshness): string {
  if (freshness === "unknown") return "No se sabe cuándo se consultó";
  const when = formatRelativeDate(observedAt);
  if (freshness === "stale") return `Consultado ${when}; puede haber cambiado`;
  if (freshness === "aging") return `Consultado ${when}`;
  return `Consultado ${when}`;
}

/**
 * Traduce la situación de precio de una entrada a lo que se pinta.
 *
 * Las tres reglas que no se negocian:
 *
 * 1. **Sin precio no se inventa un cero.** Se dice que no se ha consultado.
 * 2. **Monedas distintas no se comparan.** Si el objetivo está en euros y lo
 *    único observado está en dólares, se dice exactamente eso; no se convierte
 *    con un tipo de cambio que Vindexa no tiene.
 * 3. **El importe nunca aparece solo.** Siempre lleva su moneda y la fecha en
 *    la que se observó.
 */
export function describePrice(entry: WishlistEntry, status?: WishlistPriceStatus): PriceLine {
  const targetCents = status?.targetCents ?? entry.targetPriceCents;
  const targetCurrency = status?.targetCurrency ?? entry.currency;
  // Cero no es un objetivo, es no haberlo puesto: tratarlo como cifra hacía que
  // la tarjeta dijese «Objetivo 0,00» junto a «Precio desconocido».
  const target =
    targetCents === undefined || targetCents <= 0
      ? undefined
      : formatCents(targetCents, targetCurrency);
  const price = status?.price;
  const others = status?.otherCurrencies ?? [];

  if (!price) {
    const verdict = others.length
      ? `Sólo hay precio en ${others.join(", ")}${
          targetCurrency ? `, y tu objetivo está en ${targetCurrency}` : ""
        }: no se comparan.`
      : target
        ? `Objetivo ${target}. Todavía no se ha consultado el precio.`
        : "Sin precio objetivo ni precio consultado.";
    return {
      observed: "Sin consultar",
      freshness: "missing",
      verdict,
      meetsTarget: false,
    };
  }

  const verdict = !status?.comparable
    ? target
      ? `Tu objetivo está en ${targetCurrency} y el precio observado en ${price.currency}: no se comparan.`
      : "Sin precio objetivo con el que comparar."
    : status.meetsTarget
      ? `Por debajo de tu objetivo de ${target}.`
      : `Faltan ${formatCents(Math.abs(status.differenceCents ?? 0), price.currency)} para tu objetivo de ${target}.`;

  // Los campos opcionales se omiten en lugar de ir a `undefined`: el proyecto
  // compila con `exactOptionalPropertyTypes`, y «ausente» y «undefined» no son
  // lo mismo cuando lo que se decide es si se pinta o no.
  return {
    amount: formatCents(price.finalCents, price.currency),
    ...(price.initialCents > price.finalCents
      ? { reference: formatCents(price.initialCents, price.currency) }
      : {}),
    ...(price.discountPercent > 0 ? { discount: `−${price.discountPercent} %` } : {}),
    ...(price.lowestCents < price.finalCents
      ? { lowest: formatCents(price.lowestCents, price.currency) }
      : {}),
    observed: observedLabel(price.observedAt, price.freshness),
    freshness: price.freshness,
    verdict,
    meetsTarget: status?.meetsTarget ?? false,
  };
}

export interface PriceCoverage {
  total: number;
  withPrice: number;
  stale: number;
  meetingTarget: number;
  /** Cifra principal del encabezado. */
  headline: string;
  /** Lo que matiza la cifra. Vacío sólo cuando no hay nada que matizar. */
  caveat: string;
  /** Juegos con descuento vigente según el último precio observado. */
  onSale: number;
}

/**
 * Cuánto de la lista tiene precio conocido.
 *
 * Existe porque la cifra de arriba de la pantalla no puede sugerir que la lista
 * esté cubierta cuando no lo está: si de treinta juegos sólo doce tienen precio,
 * se dice «12 de 30», no «12».
 */
export function summarizePrices(statuses: readonly WishlistPriceStatus[]): PriceCoverage {
  const total = statuses.length;
  const withPrice = statuses.filter((status) => status.price).length;
  const stale = statuses.filter(
    (status) => status.price?.freshness === "stale" || status.price?.freshness === "unknown",
  ).length;
  const meetingTarget = statuses.filter((status) => status.comparable && status.meetsTarget).length;
  const onSale = statuses.filter((status) => (status.price?.discountPercent ?? 0) > 0).length;

  const caveats: string[] = [];
  if (withPrice < total) {
    const missing = total - withPrice;
    caveats.push(
      missing === 1
        ? "1 juego sin precio consultado"
        : `${missing.toLocaleString("es-ES")} juegos sin precio consultado`,
    );
  }
  if (stale > 0) {
    caveats.push(
      stale === 1
        ? "1 precio caducado que puede haber cambiado"
        : `${stale.toLocaleString("es-ES")} precios caducados que pueden haber cambiado`,
    );
  }

  return {
    total,
    withPrice,
    stale,
    meetingTarget,
    headline: total === 0 ? "Sin precios" : `${withPrice} de ${total} con precio`,
    caveat: caveats.length ? `${caveats.join("; ")}.` : "",
    onSale,
  };
}

/* --- Ofertas vigentes ---------------------------------------------------- */

/** Una entrada de la lista que ahora mismo está rebajada. */
export interface WishlistOffer {
  appId: number;
  title: string;
  discountPercent: number;
  finalCents: number;
  initialCents: number;
  currency: string;
  /** Cuánto falta para el objetivo. Negativo significa que ya lo cumple. */
  differenceCents?: number;
  meetsTarget: boolean;
  /** El precio observado está caducado y puede haber cambiado. */
  stale: boolean;
}

/**
 * Ofertas vigentes de la lista, de mayor descuento a menor.
 *
 * Existe porque con mil quinientos deseados repartidos en cuatro carriles nadie
 * encuentra una rebaja mirando: la razón de tener precios es poder localizar lo
 * que ha bajado, y eso exige una lista aparte, no un dato escondido en cada
 * tarjeta.
 *
 * El desempate es el ahorro absoluto y después el título, para que dos juegos
 * con el mismo porcentaje no bailen entre recargas.
 */
export function collectOffers(
  entries: readonly WishlistEntry[],
  statuses: readonly WishlistPriceStatus[],
): WishlistOffer[] {
  const porAppId = new Map(statuses.map((status) => [status.appId, status] as const));
  const ofertas: WishlistOffer[] = [];
  for (const entry of entries) {
    const status = porAppId.get(entry.game.appId);
    const price = status?.price;
    if (!price || price.discountPercent <= 0) continue;
    ofertas.push({
      appId: entry.game.appId,
      title: entry.game.title,
      discountPercent: price.discountPercent,
      finalCents: price.finalCents,
      initialCents: price.initialCents,
      currency: price.currency,
      ...(status?.differenceCents === undefined ? {} : { differenceCents: status.differenceCents }),
      meetsTarget: Boolean(status?.comparable && status.meetsTarget),
      stale: price.freshness === "stale" || price.freshness === "unknown",
    });
  }
  return ofertas.sort(
    (izquierda, derecha) =>
      derecha.discountPercent - izquierda.discountPercent ||
      derecha.initialCents -
        derecha.finalCents -
        (izquierda.initialCents - izquierda.finalCents) ||
      izquierda.title.localeCompare(derecha.title, "es"),
  );
}

/* --- Vídeos -------------------------------------------------------------- */

export interface VideoKindMeta {
  id: VideoKind;
  label: string;
}

export const VIDEO_KINDS = [
  { id: "gameplay", label: "Gameplay" },
  { id: "review", label: "Análisis" },
  { id: "impressions", label: "Impresiones" },
  { id: "trailer", label: "Tráiler" },
  { id: "guide", label: "Guía" },
] as const satisfies readonly VideoKindMeta[];

const VIDEO_KIND_BY_ID = new Map<VideoKind, VideoKindMeta>(
  VIDEO_KINDS.map((kind) => [kind.id, kind] as const),
);

export function videoKindLabel(kind: VideoKind): string {
  return VIDEO_KIND_BY_ID.get(kind)?.label ?? kind;
}

export interface VideoGroup {
  kind: VideoKind;
  label: string;
  items: GameVideo[];
}

/**
 * Los vídeos se agrupan siempre por tipo porque el backend reordena **por
 * tipo**: `reorder_game_videos` recibe un `kind`. Una lista mezclada dejaría
 * los botones de subir y bajar sin una secuencia que persistir.
 */
export function groupVideosByKind(videos: readonly GameVideo[]): VideoGroup[] {
  return VIDEO_KINDS.flatMap(({ id, label }) => {
    const items = videos
      .filter((video) => video.kind === id)
      .sort((left, right) => left.position - right.position);
    return items.length ? [{ kind: id, label, items }] : [];
  });
}

/* --- Listas curadas ------------------------------------------------------ */

export interface CuratedKindMeta {
  id: CuratedListKind;
  label: string;
  hint: string;
}

export const CURATED_KINDS = [
  {
    id: "manual",
    label: "Selección",
    hint: "Una selección personal, ordenada a mano.",
  },
  {
    id: "wishlist",
    label: "Deseados",
    hint: "Candidatos a compra reunidos con un criterio propio.",
  },
  {
    id: "backlog",
    label: "Pendientes",
    hint: "Lo que quieres terminar, en el orden en que piensas hacerlo.",
  },
  {
    id: "showcase",
    label: "Vitrina",
    hint: "Lo que enseñarías a alguien para explicar qué te gusta.",
  },
] as const satisfies readonly CuratedKindMeta[];

const CURATED_KIND_BY_ID = new Map<CuratedListKind, CuratedKindMeta>(
  CURATED_KINDS.map((kind) => [kind.id, kind] as const),
);

export function curatedKindLabel(kind: CuratedListKind): string {
  return CURATED_KIND_BY_ID.get(kind)?.label ?? kind;
}

/**
 * Acentos que la pantalla sabe pintar.
 *
 * `DESIGN.md` limita la paleta a las variables ya definidas, así que aquí solo
 * se ofrecen los acentos que tienen un token propio y distinguible. El azul de
 * acción (`--v-cyan-strong`) queda fuera a propósito: está reservado a lo que
 * se puede pulsar y una franja decorativa no lo es. Los acentos que llegan de
 * una lista creada en otra parte se pintan con el neutro en lugar de forzar un
 * color que este sistema no tiene.
 */
export const CURATED_ACCENTS = [
  { id: "cyan", label: "Cian", token: "var(--v-cyan)" },
  { id: "teal", label: "Verde", token: "var(--v-success)" },
  { id: "lime", label: "Lima", token: "var(--v-lime)" },
  { id: "amber", label: "Ámbar", token: "var(--v-warning)" },
  { id: "slate", label: "Neutro", token: "var(--v-muted)" },
] as const satisfies readonly { id: CuratedAccent; label: string; token: string }[];

export function accentToken(accent: CuratedAccent): string {
  const known = CURATED_ACCENTS.find((option) => option.id === accent);
  if (known) return known.token;
  return accent === "blue" ? "var(--v-cyan)" : "var(--v-muted)";
}

export function gameCountLabel(total: number): string {
  return total === 1 ? "1 juego" : `${total.toLocaleString("es-ES")} juegos`;
}

/* --- Movimiento del tablero ---------------------------------------------- */

export function findEntryBucket(
  buckets: readonly WishlistBucket[],
  appId: number,
): WishlistBucketId | undefined {
  return buckets.find((bucket) => bucket.items.some((item) => item.game.appId === appId))?.bucket;
}

export function findEntry(buckets: readonly WishlistBucket[], appId: number) {
  for (const bucket of buckets) {
    const entry = bucket.items.find((item) => item.game.appId === appId);
    if (entry) return entry;
  }
  return undefined;
}

/**
 * Traslado optimista de una entrada dentro del tablero.
 *
 * Se aplica antes de que el backend conteste para que la tarjeta no vuelva al
 * carril de origen durante el viaje de ida y vuelta; si la llamada falla, la
 * pantalla restaura la instantánea anterior y lo dice. `position` se recalcula
 * aquí porque el orden que ve la persona es el que se envía a `reorder`.
 */
export function applyBoardMove(
  buckets: readonly WishlistBucket[],
  appId: number,
  targetBucket: WishlistBucketId,
  beforeAppId?: number,
): WishlistBucket[] {
  const moving = findEntry(buckets, appId);
  if (!moving) return buckets.map((bucket) => ({ ...bucket, items: [...bucket.items] }));

  const stripped = buckets.map((bucket) => ({
    ...bucket,
    items: bucket.items.filter((item) => item.game.appId !== appId),
  }));

  return stripped.map((bucket) => {
    if (bucket.bucket !== targetBucket) {
      return { ...bucket, total: bucket.items.length, items: renumber(bucket.items) };
    }
    const anchor =
      beforeAppId === undefined
        ? -1
        : bucket.items.findIndex((item) => item.game.appId === beforeAppId);
    const items = [...bucket.items];
    const placed = { ...moving, bucket: targetBucket };
    if (anchor < 0) items.push(placed);
    else items.splice(anchor, 0, placed);
    return { ...bucket, items: renumber(items), total: items.length };
  });
}

function renumber(items: readonly WishlistEntry[]): WishlistEntry[] {
  return items.map((item, index) =>
    item.position === index ? item : { ...item, position: index },
  );
}
