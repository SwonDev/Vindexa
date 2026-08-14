CREATE TABLE IF NOT EXISTS game_reminders (
    id TEXT PRIMARY KEY,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    due_at TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_game_reminders_pending
    ON game_reminders(completed_at, due_at ASC, app_id);

CREATE TABLE IF NOT EXISTS game_metadata_observations (
    id TEXT PRIMARY KEY,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    is_early_access INTEGER CHECK (is_early_access IS NULL OR is_early_access IN (0, 1)),
    release_date TEXT,
    source_fetched_at TEXT NOT NULL,
    observed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(app_id, source_fetched_at)
);

CREATE INDEX IF NOT EXISTS idx_metadata_observations_game
    ON game_metadata_observations(app_id, observed_at DESC);

CREATE TABLE IF NOT EXISTS discovery_events (
    id TEXT PRIMARY KEY,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('early_access_changed', 'release_date_changed')),
    previous_value TEXT,
    current_value TEXT,
    observed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_discovery_events_recent
    ON discovery_events(observed_at DESC, app_id);

CREATE INDEX IF NOT EXISTS idx_recommendation_history_recent
    ON recommendation_history(created_at DESC, dismissed, app_id);
