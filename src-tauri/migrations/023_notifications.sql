-- Avisos programados por la persona usuaria y eventos oficiales detectados en
-- fuentes de Steam ya utilizadas por Vindexa (noticias, cambios de acceso
-- anticipado y de fecha de lanzamiento).
CREATE TABLE IF NOT EXISTS notification_rules (
    id TEXT PRIMARY KEY,
    app_id INTEGER REFERENCES games(app_id) ON DELETE CASCADE,
    kind TEXT NOT NULL
        CHECK (kind IN ('manual', 'release_date', 'early_access_exit', 'official_news',
                        'dlc_release', 'reminder_digest')),
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    scheduled_for TEXT,
    repeat_rule TEXT NOT NULL DEFAULT 'none'
        CHECK (repeat_rule IN ('none', 'daily', 'weekly', 'monthly')),
    lead_minutes INTEGER NOT NULL DEFAULT 0 CHECK (lead_minutes >= 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_fired_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_notification_rules_due
    ON notification_rules(enabled, scheduled_for ASC, id ASC);
CREATE INDEX IF NOT EXISTS idx_notification_rules_game
    ON notification_rules(app_id, kind, id);

CREATE TABLE IF NOT EXISTS notification_events (
    id TEXT PRIMARY KEY,
    rule_id TEXT REFERENCES notification_rules(id) ON DELETE SET NULL,
    app_id INTEGER REFERENCES games(app_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'info'
        CHECK (severity IN ('info', 'success', 'warning', 'critical')),
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    read_at TEXT,
    dismissed_at TEXT,
    dedupe_key TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_notification_events_dedupe
    ON notification_events(dedupe_key)
    WHERE dedupe_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_notification_events_inbox
    ON notification_events(dismissed_at, occurred_at DESC, id);
