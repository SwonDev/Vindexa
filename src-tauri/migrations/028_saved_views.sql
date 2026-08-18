-- Vistas guardadas de biblioteca.
--
-- Una vista congela una combinación completa —ámbito, búsqueda, filtros, orden,
-- agrupación y modo de presentación— bajo un nombre. A diferencia de los
-- presets de otras aplicaciones, varias vistas pueden **combinarse**: aplicar
-- una segunda sobre la primera interseca sus filtros en lugar de reemplazarlos.
CREATE TABLE IF NOT EXISTS saved_views (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL DEFAULT 'bookmark',
    accent TEXT NOT NULL DEFAULT 'cyan',
    -- Instantánea completa de la consulta, en el mismo formato que usa la
    -- interfaz. Se guarda como JSON porque el conjunto de filtros evoluciona y
    -- una columna por filtro obligaría a migrar en cada añadido.
    query_json TEXT NOT NULL DEFAULT '{}',
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    use_count INTEGER NOT NULL DEFAULT 0 CHECK (use_count >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_saved_views_name
    ON saved_views(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_saved_views_order
    ON saved_views(pinned DESC, position ASC, id ASC);
CREATE INDEX IF NOT EXISTS idx_saved_views_recent
    ON saved_views(last_used_at DESC)
    WHERE last_used_at IS NOT NULL;
