-- Precio observado de un juego y archivado de biblioteca.
--
-- ## Precios: Vindexa no conoce «el precio»
--
-- Conoce **precios observados**: cada uno con su moneda, el país con el que se
-- consultó y el instante exacto en que se miró. Por eso el modelo separa dos
-- cosas que suelen mezclarse en una sola tabla:
--
-- - `game_prices` guarda el último estado conocido por juego y moneda. Es lo
--   que la interfaz enseña, y lleva siempre `observed_at` al lado para poder
--   decir cuándo se miró y que puede haber cambiado.
-- - `game_price_history` guarda una fila **por cada cambio**, no por cada
--   consulta. Es la serie con la que se dibuja la evolución.
--
-- Consultar mil quinientos juegos a diario sin que el precio se mueva no debe
-- producir medio millón de filas al año: la frescura vive en
-- `game_prices.observed_at` y la curva en el historial.
--
-- La moneda forma parte de la clave primaria porque dos importes en monedas
-- distintas no son ni comparables ni sumables. El resto del producto ya se
-- niega a mezclarlas y aquí el esquema lo hace imposible por construcción.
--
-- `lowest_cents` es el mínimo que **Vindexa ha visto**, no el mínimo histórico
-- real del juego. Se recalcula desde `game_price_history` dentro de la misma
-- transacción que inserta la observación, de modo que nunca puede afirmar un
-- mínimo que ya no se pueda demostrar con una fila del historial.
CREATE TABLE IF NOT EXISTS game_prices (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    currency TEXT NOT NULL
        CHECK (length(currency) = 3 AND currency = upper(currency)),
    -- El `cc` con el que se pidió la ficha. Sin él, la moneda observada no es
    -- reproducible: la misma consulta desde otro país devuelve otra.
    country_code TEXT
        CHECK (
            country_code IS NULL
            OR (length(country_code) = 2 AND country_code = upper(country_code))
        ),
    final_cents INTEGER NOT NULL CHECK (final_cents >= 0),
    -- Precio de referencia (sin descuento) tal y como lo publica la tienda.
    initial_cents INTEGER NOT NULL CHECK (initial_cents >= 0),
    discount_percent INTEGER NOT NULL DEFAULT 0
        CHECK (discount_percent BETWEEN 0 AND 100),
    lowest_cents INTEGER NOT NULL CHECK (lowest_cents >= 0),
    lowest_observed_at TEXT NOT NULL,
    -- Cuándo se vio por primera vez el importe vigente. Permite decir «bajó
    -- hace dos días» sin recorrer el historial.
    changed_at TEXT NOT NULL,
    -- Cuándo se confirmó por última vez. Se actualiza en cada consulta aunque
    -- el importe no cambie: es la marca de frescura, no la de cambio.
    observed_at TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'steam_store'
        CHECK (source IN ('steam_store', 'manual')),
    PRIMARY KEY (app_id, currency)
);

-- Elegir a quién se le vuelve a preguntar el precio: los más rancios primero.
CREATE INDEX IF NOT EXISTS idx_game_prices_staleness
    ON game_prices(observed_at ASC, app_id ASC);

CREATE TABLE IF NOT EXISTS game_price_history (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    currency TEXT NOT NULL
        CHECK (length(currency) = 3 AND currency = upper(currency)),
    -- Instante en el que ese importe se vio **por primera vez**. La clave
    -- primaria ya ordena la serie, así que no hace falta un índice aparte.
    observed_at TEXT NOT NULL,
    final_cents INTEGER NOT NULL CHECK (final_cents >= 0),
    initial_cents INTEGER NOT NULL CHECK (initial_cents >= 0),
    discount_percent INTEGER NOT NULL DEFAULT 0
        CHECK (discount_percent BETWEEN 0 AND 100),
    source TEXT NOT NULL DEFAULT 'steam_store'
        CHECK (source IN ('steam_store', 'manual')),
    PRIMARY KEY (app_id, currency, observed_at)
);

-- ## Archivado
--
-- «No me lo enseñes más» es distinto de un estado. Un estado dice en qué punto
-- de la relación con el juego estás («jugando», «pendiente», «abandonado»);
-- archivar dice que ese juego no debe ocupar sitio en la vista por defecto,
-- sin afirmar nada sobre él.
--
-- Vive en su propia tabla y no como columna de `game_personal` por tres
-- motivos: no es parte del estado personal y mezclarlos invita justo a la
-- confusión que el archivado viene a resolver; ausencia de fila es el «no
-- archivado» natural, sin tercer valor que interpretar; y la tabla es pequeña
-- (unos cientos de filas frente a miles), así que filtrar con `NOT EXISTS`
-- cuesta una búsqueda por índice y no ensancha la tabla caliente.
--
-- Archivar nunca borra: la fila de `games` y la de `game_personal` siguen
-- intactas, y desarchivar es un `DELETE` de una única fila.
CREATE TABLE IF NOT EXISTS game_archive (
    app_id INTEGER PRIMARY KEY REFERENCES games(app_id) ON DELETE CASCADE,
    -- Por qué se archivó, si la persona quiso decirlo. Nunca se rellena sola.
    reason TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 200),
    archived_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_game_archive_recent
    ON game_archive(archived_at DESC, app_id ASC);
