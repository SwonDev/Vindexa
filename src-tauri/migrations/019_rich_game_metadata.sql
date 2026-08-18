-- Metadatos enriquecidos para reestructurar las descripciones de ficha y
-- disponer de arte de mayor resolución con procedencia oficial verificable.
ALTER TABLE games ADD COLUMN detailed_description TEXT;
ALTER TABLE games ADD COLUMN about_the_game TEXT;
ALTER TABLE games ADD COLUMN supported_languages TEXT;
ALTER TABLE games ADD COLUMN website_url TEXT;
ALTER TABLE games ADD COLUMN metacritic_score INTEGER
    CHECK (metacritic_score IS NULL OR metacritic_score BETWEEN 0 AND 100);
ALTER TABLE games ADD COLUMN metacritic_url TEXT;
ALTER TABLE games ADD COLUMN required_age INTEGER
    CHECK (required_age IS NULL OR required_age >= 0);
ALTER TABLE games ADD COLUMN controller_support TEXT;
ALTER TABLE games ADD COLUMN background_url TEXT;
ALTER TABLE games ADD COLUMN library_hero_url TEXT;
ALTER TABLE games ADD COLUMN library_logo_url TEXT;
ALTER TABLE games ADD COLUMN logo_position_json TEXT;

CREATE TABLE IF NOT EXISTS game_media (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    media_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('screenshot', 'movie')),
    thumbnail_url TEXT,
    full_url TEXT,
    alt_url TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (app_id, media_id)
);

CREATE INDEX IF NOT EXISTS idx_game_media_order
    ON game_media(app_id, kind, position, media_id);
