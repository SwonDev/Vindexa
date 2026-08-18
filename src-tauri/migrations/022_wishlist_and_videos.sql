-- Lista de deseados propia (independiente de la de Steam) y vídeos asociados a
-- un juego. Los vídeos guardan sólo el identificador del proveedor: la
-- reproducción se hace con `youtube-nocookie.com` y sin cookies de terceros.
CREATE TABLE IF NOT EXISTS wishlist_entries (
    app_id INTEGER PRIMARY KEY REFERENCES games(app_id) ON DELETE CASCADE,
    bucket TEXT NOT NULL DEFAULT 'considering'
        CHECK (bucket IN ('buying_now', 'waiting_sale', 'considering', 'watching')),
    priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 5),
    position INTEGER NOT NULL DEFAULT 0,
    note TEXT NOT NULL DEFAULT '',
    target_price_cents INTEGER
        CHECK (target_price_cents IS NULL OR target_price_cents >= 0),
    currency TEXT,
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_wishlist_order
    ON wishlist_entries(bucket, priority DESC, position ASC, app_id ASC);

CREATE TABLE IF NOT EXISTS game_videos (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    video_id TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'youtube'
        CHECK (provider IN ('youtube', 'steam')),
    kind TEXT NOT NULL DEFAULT 'gameplay'
        CHECK (kind IN ('gameplay', 'review', 'impressions', 'trailer', 'guide')),
    title TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL DEFAULT '',
    duration_seconds INTEGER
        CHECK (duration_seconds IS NULL OR duration_seconds >= 0),
    published_at TEXT,
    thumbnail_url TEXT,
    source TEXT NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'store')),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (app_id, provider, video_id)
);

CREATE INDEX IF NOT EXISTS idx_game_videos_order
    ON game_videos(app_id, kind, position ASC, video_id ASC);
