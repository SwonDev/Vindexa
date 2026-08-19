-- Los juegos que Epic regala cada semana.
--
-- Se guardan por dos motivos, y ninguno es la caché:
--
-- 1. **Avisar una sola vez.** `notification_events` exige que todo aviso nazca
--    de una fila que ya existía; sin esta tabla no habría de dónde derivarlo, y
--    sin `notified_at` el mismo regalo avisaría en cada arranque.
-- 2. **Recordar lo descartado.** Quien no quiere un regalo lo dice una vez.
--
-- La promoción caduca: `ends_at` es lo que separa «gratis ahora» de «lo fue».
-- Sin fecha de fin no se afirma que caduque pronto; la columna admite nulo a
-- propósito.
CREATE TABLE IF NOT EXISTS epic_free_offers (
    -- Identificador de oferta de Epic. Es estable entre respuestas, que es lo
    -- que permite no avisar dos veces del mismo regalo.
    offer_id TEXT PRIMARY KEY CHECK (length(trim(offer_id)) > 0),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT NOT NULL DEFAULT '',
    store_url TEXT NOT NULL CHECK (store_url LIKE 'https://store.epicgames.com/%'),
    image_url TEXT CHECK (image_url IS NULL OR image_url LIKE 'https://%'),
    state TEXT NOT NULL CHECK (state IN ('current', 'upcoming')),
    starts_at TEXT,
    ends_at TEXT,
    -- En la unidad mínima de la moneda, como el resto de precios de Vindexa.
    original_price_cents INTEGER
        CHECK (original_price_cents IS NULL OR original_price_cents >= 0),
    currency TEXT
        CHECK (currency IS NULL OR (length(currency) = 3 AND currency = upper(currency))),
    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- Cuándo se avisó de este regalo. Nulo mientras no se haya avisado.
    notified_at TEXT,
    -- Cuándo se descartó a mano. Nulo mientras siga interesando.
    dismissed_at TEXT
);

-- Lo vigente primero y lo que antes acaba antes: es el orden en que se enseña.
CREATE INDEX IF NOT EXISTS idx_epic_free_window
    ON epic_free_offers(state, ends_at ASC, offer_id ASC);
