-- Las ofertas dejan de ser sólo de Steam.
--
-- # Por qué se rehace la tabla
--
-- `store_deals` nació ayer con `app_id` de Steam como clave. GOG identifica sus
-- productos con números suyos, que no son AppID: guardarlos en la misma columna
-- sería confiar en que dos catálogos distintos nunca coincidan en un número, y
-- eso no es una garantía, es una apuesta.
--
-- La tabla tiene horas de vida y su contenido se vuelve a traer solo en cada
-- pasada, así que rehacerla no pierde nada que no se recupere en seis horas.
--
-- # Por qué GOG
--
-- Vende **sin DRM por definición**, que es justamente lo que esta biblioteca
-- mira. Y su catálogo público trae en una sola petición lo que en Steam cuesta
-- una ficha por juego: género, estudio, imagen y enlace.
DROP TABLE IF EXISTS store_deals;

CREATE TABLE store_deals (
    -- De qué tienda es. Sin esto, dos catálogos comparten espacio de nombres.
    store TEXT NOT NULL CHECK (store IN ('steam', 'gog')),
    -- Identificador **en esa tienda**, como texto: GOG usa números largos y
    -- Steam AppID, y compararlos como números sería mezclarlos.
    external_id TEXT NOT NULL CHECK (length(trim(external_id)) > 0),

    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    -- Adónde lleva «abrir en la tienda». Cada una tiene la suya.
    store_url TEXT NOT NULL CHECK (store_url LIKE 'https://%'),
    image_url TEXT CHECK (image_url IS NULL OR image_url LIKE 'https://%'),

    -- AppID de Steam cuando lo hay. Es lo que permite enseñar sus capturas en la
    -- vista rápida; en GOG es nulo y la vista se queda con la imagen de arriba.
    app_id INTEGER CHECK (app_id IS NULL OR app_id > 0),

    final_cents INTEGER NOT NULL CHECK (final_cents >= 0),
    initial_cents INTEGER NOT NULL CHECK (initial_cents >= 0),
    discount_percent INTEGER NOT NULL DEFAULT 0
        CHECK (discount_percent BETWEEN 0 AND 100),
    currency TEXT NOT NULL
        CHECK (length(currency) = 3 AND currency = upper(currency)),

    source TEXT NOT NULL DEFAULT 'specials'
        CHECK (source IN ('specials', 'top_sellers', 'new_releases', 'discounted')),

    genres_json TEXT NOT NULL DEFAULT '[]',
    categories_json TEXT NOT NULL DEFAULT '[]',
    developer TEXT,
    publisher TEXT,
    -- Nulo mientras no se sepan los rasgos. En GOG llegan con la propia oferta.
    facets_fetched_at TEXT,

    match_score REAL,
    match_reason TEXT NOT NULL DEFAULT '',

    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    dismissed_at TEXT,

    PRIMARY KEY (store, external_id)
);

CREATE INDEX IF NOT EXISTS idx_store_deals_ranking
    ON store_deals(dismissed_at, match_score DESC, discount_percent DESC, store, external_id);
