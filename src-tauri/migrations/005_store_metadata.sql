ALTER TABLE games ADD COLUMN short_description TEXT;
ALTER TABLE games ADD COLUMN metadata_status TEXT NOT NULL DEFAULT 'pending'
    CHECK (metadata_status IN ('pending', 'success', 'unavailable', 'failed'));
ALTER TABLE games ADD COLUMN metadata_fetched_at TEXT;

CREATE INDEX IF NOT EXISTS idx_games_metadata_refresh
    ON games(metadata_status, metadata_fetched_at);
