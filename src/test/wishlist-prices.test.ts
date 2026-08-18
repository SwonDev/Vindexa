import { describe, expect, it } from "vitest";
import { describePrice, observedLabel, summarizePrices } from "@/features/wishlist/wishlist-model";
import type { GamePrice, GameSummary, WishlistEntry, WishlistPriceStatus } from "@/lib/types";

/**
 * Presentación del precio observado en Deseados.
 *
 * Ninguna prueba de aquí toca la red ni el reloj del backend: la vigencia llega
 * ya calculada en `freshness`, que es justo lo que permite comprobar las reglas
 * de honestidad con datos fijos.
 */

function game(appId: number, title: string): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: false,
    statusId: "backlog",
    statusName: "Pendiente",
    statusColor: "#5CAAC1",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 0,
    collectionIds: [],
  };
}

function entry(extra: Partial<WishlistEntry> = {}): WishlistEntry {
  return {
    game: game(10, "Juego de prueba"),
    bucket: "waiting_sale",
    priority: 0,
    position: 0,
    note: "",
    addedAt: "2026-08-01T10:00:00Z",
    updatedAt: "2026-08-01T10:00:00Z",
    ...extra,
  };
}

function price(extra: Partial<GamePrice> = {}): GamePrice {
  return {
    appId: 10,
    currency: "EUR",
    countryCode: "ES",
    finalCents: 2999,
    initialCents: 5999,
    discountPercent: 50,
    lowestCents: 2999,
    lowestObservedAt: "2026-08-01T10:00:00Z",
    changedAt: "2026-08-01T10:00:00Z",
    observedAt: "2026-08-01T10:00:00Z",
    source: "steam_store",
    freshness: "fresh",
    ageMinutes: 30,
    ...extra,
  };
}

function status(extra: Partial<WishlistPriceStatus> = {}): WishlistPriceStatus {
  return {
    appId: 10,
    otherCurrencies: [],
    comparable: false,
    meetsTarget: false,
    ...extra,
  };
}

describe("precio observado en la tarjeta", () => {
  it("sin observación no inventa un cero: dice que no se ha consultado", () => {
    const line = describePrice(entry({ targetPriceCents: 2999, currency: "EUR" }));
    expect(line.amount).toBeUndefined();
    expect(line.freshness).toBe("missing");
    expect(line.observed).toBe("Sin consultar");
    expect(line.verdict).toContain("Todavía no se ha consultado");
    expect(line.meetsTarget).toBe(false);
  });

  it("sin objetivo ni precio lo dice sin rodeos", () => {
    const line = describePrice(entry());
    expect(line.verdict).toBe("Sin precio objetivo ni precio consultado.");
  });

  it("un objetivo en euros y un precio en dólares no se comparan", () => {
    const line = describePrice(
      entry({ targetPriceCents: 2999, currency: "EUR" }),
      status({
        targetCents: 2999,
        targetCurrency: "EUR",
        otherCurrencies: ["USD"],
        comparable: false,
      }),
    );
    expect(line.amount).toBeUndefined();
    expect(line.verdict).toContain("USD");
    expect(line.verdict).toContain("no se comparan");
    // Nada de convertir con un tipo de cambio que Vindexa no tiene.
    expect(line.verdict).not.toContain("equivale");
  });

  it("con precio observado en otra moneda que la del objetivo tampoco compara", () => {
    const line = describePrice(
      entry({ targetPriceCents: 2999, currency: "EUR" }),
      status({
        targetCents: 2999,
        targetCurrency: "EUR",
        price: price({ currency: "USD" }),
        comparable: false,
      }),
    );
    // El importe se pinta en su moneda real («29,99 US$»), nunca en la del
    // objetivo: reetiquetarlo sería exactamente el dato inventado que aquí se
    // persigue.
    expect(line.amount).toContain("29,99");
    expect(line.amount).toContain("US$");
    expect(line.amount).not.toContain("€");
    expect(line.verdict).toContain("EUR");
    expect(line.verdict).toContain("USD");
    expect(line.verdict).toContain("no se comparan");
    expect(line.meetsTarget).toBe(false);
  });

  it("un precio por debajo del objetivo lo dice y marca la tarjeta", () => {
    const line = describePrice(
      entry({ targetPriceCents: 3499, currency: "EUR" }),
      status({
        targetCents: 3499,
        targetCurrency: "EUR",
        price: price(),
        comparable: true,
        differenceCents: -500,
        meetsTarget: true,
      }),
    );
    expect(line.meetsTarget).toBe(true);
    expect(line.verdict).toContain("Por debajo de tu objetivo");
    expect(line.discount).toBe("−50 %");
    expect(line.reference).toContain("59,99");
  });

  it("un precio por encima del objetivo dice cuánto falta, no que esté cerca", () => {
    const line = describePrice(
      entry({ targetPriceCents: 1999, currency: "EUR" }),
      status({
        targetCents: 1999,
        targetCurrency: "EUR",
        price: price({ finalCents: 2999, discountPercent: 0, initialCents: 2999 }),
        comparable: true,
        differenceCents: 1000,
        meetsTarget: false,
      }),
    );
    expect(line.meetsTarget).toBe(false);
    expect(line.verdict).toContain("Faltan");
    expect(line.verdict).toContain("10,00");
    expect(line.discount).toBeUndefined();
    expect(line.reference).toBeUndefined();
  });

  it("enseña el mínimo visto sólo cuando es menor que el precio vigente", () => {
    const barato = describePrice(
      entry(),
      status({ price: price({ lowestCents: 999 }), comparable: false }),
    );
    expect(barato.lowest).toContain("9,99");

    const igual = describePrice(
      entry(),
      status({ price: price({ lowestCents: 2999 }), comparable: false }),
    );
    expect(igual.lowest).toBeUndefined();
  });
});

describe("vigencia de la observación", () => {
  it("una observación caducada avisa de que puede haber cambiado", () => {
    const texto = observedLabel("2026-07-01T10:00:00Z", "stale");
    expect(texto).toContain("puede haber cambiado");
  });

  it("una observación reciente no añade advertencias que no tocan", () => {
    const texto = observedLabel("2026-08-01T10:00:00Z", "fresh");
    expect(texto).toContain("Consultado");
    expect(texto).not.toContain("puede haber cambiado");
  });

  it("un sello ilegible no finge una fecha", () => {
    expect(observedLabel("no soy una fecha", "unknown")).toBe("No se sabe cuándo se consultó");
  });

  it("la tarjeta arrastra la advertencia del precio caducado", () => {
    const line = describePrice(
      entry(),
      status({ price: price({ freshness: "stale", ageMinutes: 44_640 }) }),
    );
    expect(line.freshness).toBe("stale");
    expect(line.observed).toContain("puede haber cambiado");
  });
});

describe("cobertura de precios de la lista", () => {
  it("con la lista vacía no hay cifra que dar", () => {
    const resumen = summarizePrices([]);
    expect(resumen.headline).toBe("Sin precios");
    expect(resumen.caveat).toBe("");
  });

  it("nunca sugiere que la lista esté cubierta cuando no lo está", () => {
    const resumen = summarizePrices([
      status({ appId: 1, price: price({ appId: 1 }) }),
      status({ appId: 2 }),
      status({ appId: 3 }),
    ]);
    expect(resumen.headline).toBe("1 de 3 con precio");
    expect(resumen.caveat).toContain("2 juegos sin precio consultado");
  });

  it("cuenta aparte los precios caducados", () => {
    const resumen = summarizePrices([
      status({ appId: 1, price: price({ appId: 1, freshness: "stale" }) }),
      status({ appId: 2, price: price({ appId: 2, freshness: "unknown" }) }),
      status({ appId: 3, price: price({ appId: 3 }) }),
    ]);
    expect(resumen.withPrice).toBe(3);
    expect(resumen.stale).toBe(2);
    expect(resumen.caveat).toContain("2 precios caducados");
  });

  it("con todo consultado y vigente no añade matices", () => {
    const resumen = summarizePrices([
      status({ appId: 1, price: price({ appId: 1 }) }),
      status({ appId: 2, price: price({ appId: 2 }) }),
    ]);
    expect(resumen.headline).toBe("2 de 2 con precio");
    expect(resumen.caveat).toBe("");
  });

  it("cuenta los que ya cumplen el objetivo sin mezclar los que no se comparan", () => {
    const resumen = summarizePrices([
      status({ appId: 1, price: price({ appId: 1 }), comparable: true, meetsTarget: true }),
      // Cumple «meetsTarget» por defecto en falso, pero además no es comparable:
      // una moneda distinta nunca cuenta como objetivo alcanzado.
      status({ appId: 2, price: price({ appId: 2, currency: "USD" }), comparable: false }),
    ]);
    expect(resumen.meetingTarget).toBe(1);
  });
});
