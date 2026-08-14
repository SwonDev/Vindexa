CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS steam_accounts (
    steam_id TEXT PRIMARY KEY,
    persona_name TEXT,
    avatar_url TEXT,
    profile_url TEXT,
    visibility INTEGER,
    last_sync_at TEXT,
    last_sync_status TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS statuses (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL,
    position INTEGER NOT NULL,
    built_in INTEGER NOT NULL DEFAULT 0 CHECK (built_in IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS games (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    title TEXT NOT NULL,
    icon_url TEXT,
    cover_url TEXT,
    header_url TEXT,
    playtime_minutes INTEGER NOT NULL DEFAULT 0 CHECK (playtime_minutes >= 0),
    playtime_recent_minutes INTEGER NOT NULL DEFAULT 0 CHECK (playtime_recent_minutes >= 0),
    last_played_at TEXT,
    release_date TEXT,
    developer TEXT,
    publisher TEXT,
    genres_json TEXT NOT NULL DEFAULT '[]',
    categories_json TEXT NOT NULL DEFAULT '[]',
    is_early_access INTEGER NOT NULL DEFAULT 0 CHECK (is_early_access IN (0, 1)),
    steam_deck_status TEXT,
    achievements_unlocked INTEGER,
    achievements_total INTEGER,
    is_free INTEGER NOT NULL DEFAULT 0 CHECK (is_free IN (0, 1)),
    imported_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS game_personal (
    app_id INTEGER PRIMARY KEY REFERENCES games(app_id) ON DELETE CASCADE,
    status_id TEXT NOT NULL REFERENCES statuses(id) ON UPDATE CASCADE,
    progress INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 5),
    installed INTEGER NOT NULL DEFAULT 0 CHECK (installed IN (0, 1)),
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    tracking INTEGER NOT NULL DEFAULT 0 CHECK (tracking IN (0, 1)),
    rating INTEGER CHECK (rating IS NULL OR rating BETWEEN 1 AND 10),
    estimated_minutes INTEGER CHECK (estimated_minutes IS NULL OR estimated_minutes >= 0),
    target_date TEXT,
    next_action TEXT,
    checkpoint TEXT,
    notes TEXT,
    started_at TEXT,
    completed_at TEXT,
    abandoned_at TEXT,
    manual_position INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS game_installations (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    library_path TEXT NOT NULL,
    install_path TEXT NOT NULL,
    size_on_disk INTEGER,
    build_id INTEGER,
    last_updated_at TEXT,
    is_primary INTEGER NOT NULL DEFAULT 1 CHECK (is_primary IN (0, 1)),
    PRIMARY KEY (app_id, library_path)
);

CREATE TABLE IF NOT EXISTS collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL,
    icon TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('manual', 'smart')),
    match_mode TEXT NOT NULL DEFAULT 'all' CHECK (match_mode IN ('all', 'any')),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS collection_games (
    collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (collection_id, app_id)
);

CREATE TABLE IF NOT EXISTS smart_rules (
    id TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL DEFAULT 0,
    field TEXT NOT NULL,
    operator TEXT NOT NULL,
    value_json TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS game_tags (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (app_id, tag_id)
);

CREATE TABLE IF NOT EXISTS planner_columns (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL,
    position INTEGER NOT NULL,
    wip_limit INTEGER CHECK (wip_limit IS NULL OR wip_limit > 0)
);

CREATE TABLE IF NOT EXISTS planner_items (
    column_id TEXT NOT NULL REFERENCES planner_columns(id) ON DELETE CASCADE,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    target_date TEXT,
    estimated_minutes INTEGER CHECK (estimated_minutes IS NULL OR estimated_minutes >= 0),
    PRIMARY KEY (column_id, app_id),
    UNIQUE (app_id)
);

CREATE TABLE IF NOT EXISTS game_sessions (
    id TEXT PRIMARY KEY,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    progress_before INTEGER CHECK (progress_before IS NULL OR progress_before BETWEEN 0 AND 100),
    progress_after INTEGER CHECK (progress_after IS NULL OR progress_after BETWEEN 0 AND 100),
    note TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS activity (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    app_id INTEGER REFERENCES games(app_id) ON DELETE SET NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS sync_runs (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    imported_count INTEGER NOT NULL DEFAULT 0,
    updated_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS recommendation_history (
    id TEXT PRIMARY KEY,
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    duration_minutes INTEGER,
    mood TEXT,
    dismissed INTEGER NOT NULL DEFAULT 0 CHECK (dismissed IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS openid_nonces (
    nonce TEXT PRIMARY KEY,
    used_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS image_cache (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    variant TEXT NOT NULL,
    local_path TEXT NOT NULL,
    etag TEXT,
    last_modified TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (app_id, variant)
);
