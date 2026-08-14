CREATE TABLE IF NOT EXISTS family_catalog_games (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    title TEXT NOT NULL,
    icon_url TEXT,
    cover_url TEXT,
    header_url TEXT,
    availability TEXT NOT NULL DEFAULT 'unknown'
        CHECK (availability IN ('unknown', 'confirmed')),
    discovered_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_family_catalog_title
    ON family_catalog_games(title COLLATE NOCASE, app_id);
CREATE INDEX IF NOT EXISTS idx_family_catalog_availability
    ON family_catalog_games(availability, title COLLATE NOCASE, app_id);
