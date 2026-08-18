-- Listas curadas: selecciones editoriales personales, independientes de las
-- colecciones (manuales o inteligentes). Una lista curada admite orden manual,
-- nota por juego y una pieza de vídeo asociada por entrada.
CREATE TABLE IF NOT EXISTS curated_lists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT 'manual'
        CHECK (kind IN ('manual', 'wishlist', 'backlog', 'showcase')),
    accent TEXT NOT NULL DEFAULT 'cyan',
    icon TEXT NOT NULL DEFAULT 'list',
    cover_app_id INTEGER REFERENCES games(app_id) ON DELETE SET NULL,
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_curated_lists_name
    ON curated_lists(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_curated_lists_order
    ON curated_lists(pinned DESC, position ASC, id ASC);

CREATE TABLE IF NOT EXISTS curated_list_items (
    list_id TEXT NOT NULL REFERENCES curated_lists(id) ON DELETE CASCADE,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    note TEXT NOT NULL DEFAULT '',
    highlight INTEGER NOT NULL DEFAULT 0 CHECK (highlight IN (0, 1)),
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (list_id, app_id)
);

CREATE INDEX IF NOT EXISTS idx_curated_list_items_order
    ON curated_list_items(list_id, position ASC, app_id ASC);
CREATE INDEX IF NOT EXISTS idx_curated_list_items_game
    ON curated_list_items(app_id, list_id);
