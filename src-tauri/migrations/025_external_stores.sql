-- Vinculación con tiendas externas (Epic Games Store y GOG). Vindexa lee
-- únicamente los manifiestos locales que cada cliente escribe en disco; no
-- almacena credenciales de esas tiendas ni se conecta a sus APIs privadas.
CREATE TABLE IF NOT EXISTS external_store_accounts (
    store TEXT PRIMARY KEY CHECK (store IN ('epic', 'gog')),
    display_name TEXT,
    detected_root TEXT,
    linked INTEGER NOT NULL DEFAULT 0 CHECK (linked IN (0, 1)),
    last_scan_at TEXT,
    last_scan_status TEXT
        CHECK (last_scan_status IS NULL OR last_scan_status IN ('success', 'failed', 'unavailable')),
    last_scan_error_code TEXT,
    last_scan_error_message TEXT,
    game_count INTEGER NOT NULL DEFAULT 0 CHECK (game_count >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS external_games (
    store TEXT NOT NULL CHECK (store IN ('epic', 'gog')),
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    cover_url TEXT,
    header_url TEXT,
    install_path TEXT,
    installed INTEGER NOT NULL DEFAULT 0 CHECK (installed IN (0, 1)),
    size_on_disk INTEGER CHECK (size_on_disk IS NULL OR size_on_disk >= 0),
    launch_target TEXT,
    drm_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK (drm_state IN ('unknown', 'drm_free', 'third_party_drm', 'steam_drm')),
    matched_app_id INTEGER REFERENCES games(app_id) ON DELETE SET NULL,
    match_confidence REAL NOT NULL DEFAULT 0
        CHECK (match_confidence BETWEEN 0 AND 1),
    discovered_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (store, external_id)
);

CREATE INDEX IF NOT EXISTS idx_external_games_title
    ON external_games(title COLLATE NOCASE, store, external_id);
CREATE INDEX IF NOT EXISTS idx_external_games_match
    ON external_games(matched_app_id, store)
    WHERE matched_app_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_external_games_installed
    ON external_games(installed DESC, store, title COLLATE NOCASE);
