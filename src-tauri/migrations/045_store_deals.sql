-- Ofertas de la tienda que no están en tus deseados.
--
-- # Qué son y qué no
--
-- Vindexa ya sabía decir qué está rebajado **de tu lista**. Esta tabla es lo
-- otro: lo que está rebajado en la tienda y todavía no has mirado. Sin un
-- criterio propio eso sería un escaparate, así que cada oferta se puntúa contra
-- el mismo modelo de gustos que ya ordena los próximos lanzamientos —el que se
-- calcula con tu historial, en tu ordenador y sin salir de él—.
--
-- # Por qué se guardan
--
-- Porque la puntuación necesita los géneros, las categorías y el estudio, y eso
-- son una petición por juego. Guardarlos evita repetirlas cada vez que se abre
-- la pantalla, y permite recordar lo descartado.
--
-- `match_score` nulo significa «aún no puntuado», que no es lo mismo que «no te
-- interesa»: la interfaz enseña la diferencia.
CREATE TABLE IF NOT EXISTS store_deals (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    header_url TEXT CHECK (header_url IS NULL OR header_url LIKE 'https://%'),

    -- Precio en la unidad mínima de la moneda, como el resto de Vindexa.
    final_cents INTEGER NOT NULL CHECK (final_cents >= 0),
    initial_cents INTEGER NOT NULL CHECK (initial_cents >= 0),
    discount_percent INTEGER NOT NULL DEFAULT 0
        CHECK (discount_percent BETWEEN 0 AND 100),
    currency TEXT NOT NULL
        CHECK (length(currency) = 3 AND currency = upper(currency)),

    -- De qué escaparate salió. No se mezclan: una rebaja y un superventas sin
    -- descuento son cosas distintas.
    source TEXT NOT NULL DEFAULT 'specials'
        CHECK (source IN ('specials', 'top_sellers', 'new_releases')),

    -- Rasgos para puntuar. Nulos mientras no se hayan pedido.
    genres_json TEXT NOT NULL DEFAULT '[]',
    categories_json TEXT NOT NULL DEFAULT '[]',
    developer TEXT,
    publisher TEXT,
    facets_fetched_at TEXT,

    -- Afinidad con tu historial. Nulo es «sin puntuar», no «cero».
    match_score REAL,
    match_reason TEXT NOT NULL DEFAULT '',

    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    dismissed_at TEXT
);

-- Lo mejor puntuado primero, y a igualdad, lo más rebajado.
CREATE INDEX IF NOT EXISTS idx_store_deals_ranking
    ON store_deals(dismissed_at, match_score DESC, discount_percent DESC, app_id ASC);
