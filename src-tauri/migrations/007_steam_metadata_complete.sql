ALTER TABLE games ADD COLUMN ownership_source TEXT NOT NULL DEFAULT 'owned'
    CHECK (ownership_source IN ('owned', 'family_shared', 'local'));
ALTER TABLE games ADD COLUMN family_availability TEXT NOT NULL DEFAULT 'not_applicable'
    CHECK (family_availability IN ('not_applicable', 'unknown', 'confirmed'));
ALTER TABLE games ADD COLUMN achievements_status TEXT NOT NULL DEFAULT 'pending'
    CHECK (achievements_status IN ('pending', 'success', 'unavailable', 'failed'));
ALTER TABLE games ADD COLUMN achievements_fetched_at TEXT;

CREATE INDEX IF NOT EXISTS idx_games_ownership_source
    ON games(ownership_source, family_availability, title COLLATE NOCASE, app_id);
