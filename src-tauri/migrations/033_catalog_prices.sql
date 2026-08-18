-- Precios también para los deseados que aún no se poseen.
--
-- `game_prices` y `game_price_history` nacieron con una clave foránea contra
-- `games`, cuando los deseados sólo admitían juegos de la biblioteca. Desde la
-- migración 030 un deseado puede vivir en `catalog_games` —lo normal, porque
-- una lista de deseados es de juegos que **no** se tienen—, y esa clave foránea
-- impedía guardarle un precio: justo al juego cuyo precio más importa.
--
-- La restricción se retira en lugar de duplicar las dos tablas para el
-- catálogo. Un precio se identifica por AppID de Steam y no necesita saber en
-- qué lado vive el juego; tener dos tablas gemelas obligaría a consultar y
-- mantener las dos en cada lectura, y a decidir qué pasa cuando un juego se
-- compra y cambia de lado. Sin la clave foránea, comprar un juego no mueve ni
-- una fila de precio.
--
-- A cambio, la limpieza de precios huérfanos deja de ser automática: se hace
-- en `db::pricing::forget_prices`, que ya existe y se llama al retirar un
-- deseado. Es el precio de que un dato de tienda no dependa de la biblioteca.
--
-- SQLite no permite retirar una clave foránea sin reconstruir la tabla, así que
-- se aplica el procedimiento estándar. Es seguro: ninguna otra tabla referencia
-- a estas dos, y el copiado es fila a fila sin filtro.

CREATE TABLE game_prices_nueva (
    app_id INTEGER NOT NULL,
    currency TEXT NOT NULL
        CHECK (length(currency) = 3 AND currency = upper(currency)),
    country_code TEXT
        CHECK (
            country_code IS NULL
            OR (length(country_code) = 2 AND country_code = upper(country_code))
        ),
    final_cents INTEGER NOT NULL CHECK (final_cents >= 0),
    initial_cents INTEGER NOT NULL CHECK (initial_cents >= 0),
    discount_percent INTEGER NOT NULL DEFAULT 0
        CHECK (discount_percent BETWEEN 0 AND 100),
    lowest_cents INTEGER NOT NULL CHECK (lowest_cents >= 0),
    lowest_observed_at TEXT NOT NULL,
    changed_at TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'steam_store'
        CHECK (source IN ('steam_store', 'manual')),
    PRIMARY KEY (app_id, currency)
);
INSERT INTO game_prices_nueva SELECT * FROM game_prices;
DROP TABLE game_prices;
ALTER TABLE game_prices_nueva RENAME TO game_prices;
CREATE INDEX IF NOT EXISTS idx_game_prices_staleness
    ON game_prices(observed_at ASC, app_id ASC);

CREATE TABLE game_price_history_nueva (
    app_id INTEGER NOT NULL,
    currency TEXT NOT NULL
        CHECK (length(currency) = 3 AND currency = upper(currency)),
    observed_at TEXT NOT NULL,
    final_cents INTEGER NOT NULL CHECK (final_cents >= 0),
    initial_cents INTEGER NOT NULL CHECK (initial_cents >= 0),
    discount_percent INTEGER NOT NULL DEFAULT 0
        CHECK (discount_percent BETWEEN 0 AND 100),
    source TEXT NOT NULL DEFAULT 'steam_store'
        CHECK (source IN ('steam_store', 'manual')),
    PRIMARY KEY (app_id, currency, observed_at)
);
INSERT INTO game_price_history_nueva SELECT * FROM game_price_history;
DROP TABLE game_price_history;
ALTER TABLE game_price_history_nueva RENAME TO game_price_history;
