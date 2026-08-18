-- itch.io como tercera tienda externa.
--
-- La migración 025 cerró la columna `store` con un CHECK que sólo admite
-- 'epic' y 'gog'. SQLite no permite modificar ni retirar un CHECK: la única vía
-- admitida es el procedimiento de reconstrucción que documenta la propia
-- SQLite. Se hace aquí, una vez, copiando fila por fila.
--
-- Es seguro por tres motivos comprobables:
--   1. Ninguna otra tabla del esquema referencia `external_games` ni
--      `external_store_accounts` (no hay ni una cláusula REFERENCES hacia
--      ellas), así que el renombrado no deja punteros colgando.
--   2. `migrate()` aplica cada migración dentro de una transacción: si algo
--      falla, no queda una tabla a medias.
--   3. El copiado es `INSERT INTO nueva SELECT ... FROM antigua`, columna a
--      columna y sin filtro: no se pierde ninguna fila ni ninguna corrección
--      manual de emparejado.

CREATE TABLE external_store_accounts_nueva (
    store TEXT PRIMARY KEY CHECK (store IN ('epic', 'gog', 'itch')),
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

INSERT INTO external_store_accounts_nueva (
    store, display_name, detected_root, linked, last_scan_at, last_scan_status,
    last_scan_error_code, last_scan_error_message, game_count, created_at, updated_at
)
SELECT store, display_name, detected_root, linked, last_scan_at, last_scan_status,
       last_scan_error_code, last_scan_error_message, game_count, created_at, updated_at
  FROM external_store_accounts;

DROP TABLE external_store_accounts;
ALTER TABLE external_store_accounts_nueva RENAME TO external_store_accounts;

CREATE TABLE external_games_nueva (
    store TEXT NOT NULL CHECK (store IN ('epic', 'gog', 'itch')),
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
    match_source TEXT NOT NULL DEFAULT 'automatic'
        CHECK (match_source IN ('automatic', 'manual')),
    discovered_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (store, external_id)
);

INSERT INTO external_games_nueva (
    store, external_id, title, cover_url, header_url, install_path, installed,
    size_on_disk, launch_target, drm_state, matched_app_id, match_confidence,
    match_source, discovered_at, updated_at
)
SELECT store, external_id, title, cover_url, header_url, install_path, installed,
       size_on_disk, launch_target, drm_state, matched_app_id, match_confidence,
       match_source, discovered_at, updated_at
  FROM external_games;

DROP TABLE external_games;
ALTER TABLE external_games_nueva RENAME TO external_games;

-- Los índices desaparecen con la tabla antigua: se vuelven a crear tal y como
-- los dejaron las migraciones 025 y 027.
CREATE INDEX IF NOT EXISTS idx_external_games_title
    ON external_games(title COLLATE NOCASE, store, external_id);
CREATE INDEX IF NOT EXISTS idx_external_games_match
    ON external_games(matched_app_id, store)
    WHERE matched_app_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_external_games_installed
    ON external_games(installed DESC, store, title COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_external_games_match_source
    ON external_games(store, match_source)
    WHERE match_source = 'manual';
