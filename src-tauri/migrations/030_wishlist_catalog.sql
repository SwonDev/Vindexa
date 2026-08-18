-- Catálogo: los juegos que quieres y todavía no tienes.
--
-- ## Por qué no viven en `games`
--
-- `games` es la biblioteca: lo que se posee. Su columna `ownership_source` sólo
-- admite `owned`, `family_shared` y `local`, y ninguno de los tres describe un
-- juego que no se tiene. Meterlo ahí con cualquiera de esos valores afirmaría
-- una propiedad falsa, y noventa y una consultas repartidas por el proyecto leen
-- `games` dando por hecho que cada fila es tuya: la biblioteca, los recuentos,
-- la búsqueda, el planificador, las colecciones inteligentes, la prioridad, el
-- seguimiento, el índice del agente y el emparejamiento con otras tiendas.
-- Bastaría con olvidar el filtro en una para que apareciese un juego que no
-- tienes en medio de tu biblioteca.
--
-- Por eso el catálogo es una tabla aparte sin clave foránea contra `games`. No
-- hay filtro que olvidar: lo que no está en `games` no lo puede ver ninguna
-- consulta de biblioteca. El precio de esa elección es que el catálogo no
-- hereda nada de la biblioteca —ni precios observados, ni arte cacheado, ni
-- logros—, y se paga a conciencia.
--
-- ## Qué se guarda y qué se deriva
--
-- Sólo el AppID, el nombre publicado por la tienda y de dónde salió. La portada
-- y la cabecera **no se guardan**: se derivan del AppID con las mismas funciones
-- que usa el escaneo local (`steam::local::cover_url` / `header_url`), sin una
-- sola petición de red. Guardar una URL derivable sería duplicar un dato que ya
-- se puede calcular, y arriesgarse a que las dos versiones dejen de coincidir.
--
-- Sin nombre no hay fila: un juego del que sólo se conoce el número no se puede
-- presentar sin inventarle un título.
CREATE TABLE IF NOT EXISTS catalog_games (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    source TEXT NOT NULL DEFAULT 'steam_wishlist'
        CHECK (source IN ('steam_wishlist', 'manual')),
    -- Cuándo entró en el catálogo. No es la fecha en la que se deseó el juego
    -- —esa vive en `catalog_wishlist_entries.added_at`, y puede venir de Steam—
    -- sino la primera vez que Vindexa supo de él.
    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- El deseo sobre un juego del catálogo. Columna por columna es
-- `wishlist_entries`: mismos cubos, mismos límites, mismos nombres. Esa simetría
-- no es casual, es lo que permite leer las dos tablas como una sola lista con un
-- `UNION ALL` y mover una fila de aquí a allí sin traducir nada.
CREATE TABLE IF NOT EXISTS catalog_wishlist_entries (
    app_id INTEGER PRIMARY KEY REFERENCES catalog_games(app_id) ON DELETE CASCADE,
    bucket TEXT NOT NULL DEFAULT 'considering'
        CHECK (bucket IN ('buying_now', 'waiting_sale', 'considering', 'watching')),
    priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 5),
    position INTEGER NOT NULL DEFAULT 0,
    note TEXT NOT NULL DEFAULT '',
    target_price_cents INTEGER
        CHECK (target_price_cents IS NULL OR target_price_cents >= 0),
    currency TEXT,
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_catalog_wishlist_order
    ON catalog_wishlist_entries(bucket, priority DESC, position ASC, app_id ASC);

-- ## La invariante
--
-- Un AppID está en la biblioteca **o** en el catálogo, nunca en los dos sitios.
-- Si pudiera estar en ambos, la lista de deseados enseñaría el juego dos veces y
-- no habría forma de decidir cuál de las dos filas manda.
--
-- Se defiende desde el esquema, no desde Rust, porque el catálogo tiene que
-- seguir siendo coherente con independencia de qué código escriba en `games`.
CREATE TRIGGER IF NOT EXISTS catalog_games_reject_owned
BEFORE INSERT ON catalog_games
FOR EACH ROW
WHEN EXISTS (SELECT 1 FROM games WHERE games.app_id = NEW.app_id)
BEGIN
    SELECT RAISE(ABORT, 'Ese juego ya está en la biblioteca.');
END;

-- ## Comprarlo
--
-- Cuando el juego entra en la biblioteca deja de ser catálogo, y su deseo —el
-- cubo, la nota, el precio objetivo, la prioridad, la fecha en la que se deseó—
-- tiene que sobrevivir al cambio. La fila se muda de `catalog_wishlist_entries`
-- a `wishlist_entries` en el mismo instante en que aparece en `games`, dentro de
-- la transacción de la sincronización.
--
-- Va en un disparador y no en la sincronización de Steam por dos razones: la
-- mudanza no puede depender de que quien inserte en `games` se acuerde de
-- llamarla, y así no queda ni un instante en el que el juego esté a la vez en la
-- biblioteca y en el catálogo.
--
-- `position` se calcula al final del cubo de destino en vez de conservar la del
-- catálogo: las dos numeraciones son independientes y reutilizarla colocaría el
-- juego en un sitio arbitrario de la lista de deseados de la biblioteca.
CREATE TRIGGER IF NOT EXISTS catalog_games_promote_on_library_insert
AFTER INSERT ON games
FOR EACH ROW
WHEN EXISTS (SELECT 1 FROM catalog_games WHERE catalog_games.app_id = NEW.app_id)
BEGIN
    INSERT INTO wishlist_entries(
        app_id, bucket, priority, position, note,
        target_price_cents, currency, added_at, updated_at
    )
    SELECT entry.app_id,
           entry.bucket,
           entry.priority,
           (SELECT COALESCE(MAX(existing.position) + 1, 0)
              FROM wishlist_entries existing
             WHERE existing.bucket = entry.bucket),
           entry.note,
           entry.target_price_cents,
           entry.currency,
           entry.added_at,
           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
      FROM catalog_wishlist_entries entry
     WHERE entry.app_id = NEW.app_id
       AND NOT EXISTS (
           SELECT 1 FROM wishlist_entries already WHERE already.app_id = NEW.app_id
       );
    DELETE FROM catalog_games WHERE app_id = NEW.app_id;
END;
