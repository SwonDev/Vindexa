CREATE TABLE IF NOT EXISTS steam_news_items (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    gid TEXT NOT NULL,
    title TEXT NOT NULL,
    content_preview TEXT NOT NULL DEFAULT '',
    published_at TEXT NOT NULL,
    feed_label TEXT NOT NULL,
    feed_name TEXT NOT NULL CHECK (feed_name = 'steam_community_announcements'),
    fetched_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (app_id, gid)
);

CREATE INDEX IF NOT EXISTS idx_steam_news_recent
    ON steam_news_items(published_at DESC, app_id, gid);

CREATE TABLE IF NOT EXISTS steam_news_fetch_state (
    app_id INTEGER PRIMARY KEY REFERENCES games(app_id) ON DELETE CASCADE,
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    last_attempt_at TEXT,
    last_success_at TEXT,
    next_attempt_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_error_code TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_steam_news_refresh_due
    ON steam_news_fetch_state(next_attempt_at, consecutive_failures, app_id);
