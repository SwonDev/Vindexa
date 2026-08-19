-- Capturas para la vista rápida al pasar el ratón.
--
-- # Por qué no vale `game_media`
--
-- `game_media` guarda la galería completa de un juego **de la biblioteca**: su
-- clave foránea apunta a `games`, y ahí no están los mil trescientos deseados
-- que aún no se poseen, que son justo los que se miran para decidir si compras.
-- Esta tabla admite cualquier AppID y guarda sólo la miniatura, que es lo único
-- que la vista rápida enseña.
--
-- No duplica nada: la lectura mira primero `game_media` y sólo cae aquí cuando
-- ese juego no está en la biblioteca. Cada juego vive en un sitio, no en dos.
CREATE TABLE IF NOT EXISTS preview_screenshots (
    app_id INTEGER NOT NULL CHECK (app_id > 0),
    position INTEGER NOT NULL CHECK (position >= 0),
    -- Sólo `https`: una imagen por texto plano en una ventana de aplicación es
    -- tráfico observable y un aviso del sistema.
    thumbnail_url TEXT NOT NULL CHECK (thumbnail_url LIKE 'https://%'),
    PRIMARY KEY (app_id, position)
);

-- Cuándo se preguntó por las capturas de un juego.
--
-- Sin esto, un juego sin capturas —retirado, o que nunca las publicó— se
-- preguntaría en cada pasada del ratón: cero filas y «no lo he mirado nunca»
-- son indistinguibles sin una marca aparte.
CREATE TABLE IF NOT EXISTS preview_screenshot_checks (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    checked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- Cuántas se encontraron. Cero es un dato, no un hueco.
    found INTEGER NOT NULL DEFAULT 0 CHECK (found >= 0)
);
