-- Gestión de DLC de los juegos de la biblioteca.
CREATE TABLE IF NOT EXISTS game_dlc (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    dlc_app_id INTEGER NOT NULL CHECK (dlc_app_id > 0),
    title TEXT NOT NULL DEFAULT '',
    capsule_url TEXT,
    header_url TEXT,
    short_description TEXT,
    release_date TEXT,
    is_free INTEGER NOT NULL DEFAULT 0 CHECK (is_free IN (0, 1)),
    price_cents INTEGER CHECK (price_cents IS NULL OR price_cents >= 0),
    currency TEXT,
    discount_percent INTEGER
        CHECK (discount_percent IS NULL OR discount_percent BETWEEN 0 AND 100),
    owned INTEGER NOT NULL DEFAULT 0 CHECK (owned IN (0, 1)),
    installed INTEGER NOT NULL DEFAULT 0 CHECK (installed IN (0, 1)),
    hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    metadata_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (metadata_status IN ('pending', 'success', 'unavailable', 'failed')),
    metadata_fetched_at TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (app_id, dlc_app_id)
);

CREATE INDEX IF NOT EXISTS idx_game_dlc_owned
    ON game_dlc(app_id, owned DESC, position, dlc_app_id);
CREATE INDEX IF NOT EXISTS idx_game_dlc_refresh
    ON game_dlc(metadata_status, metadata_fetched_at, dlc_app_id);
