-- Motor de prioridad dinámica: la prioridad manual (0-5) sigue mandando cuando
-- está anclada, pero Vindexa calcula además una puntuación derivada de señales
-- verificables (estado, progreso, sesiones recientes, fecha objetivo). Terminar
-- un juego baja su puntuación sin borrar la intención manual de la persona.
ALTER TABLE game_personal ADD COLUMN priority_score REAL NOT NULL DEFAULT 0;
ALTER TABLE game_personal ADD COLUMN priority_locked INTEGER NOT NULL DEFAULT 0
    CHECK (priority_locked IN (0, 1));
ALTER TABLE game_personal ADD COLUMN priority_computed_at TEXT;
ALTER TABLE game_personal ADD COLUMN priority_reason TEXT;

CREATE INDEX IF NOT EXISTS idx_personal_priority_score
    ON game_personal(priority_locked DESC, priority_score DESC, app_id ASC);

CREATE TABLE IF NOT EXISTS priority_signals (
    app_id INTEGER NOT NULL REFERENCES games(app_id) ON DELETE CASCADE,
    signal TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 0,
    detail TEXT NOT NULL DEFAULT '',
    computed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (app_id, signal)
);

CREATE INDEX IF NOT EXISTS idx_priority_signals_weight
    ON priority_signals(app_id, weight DESC, signal);

-- Modelo de gustos: pesos aprendidos por faceta a partir del comportamiento
-- local (tiempo jugado, terminados, descartes). Nunca sale del equipo.
CREATE TABLE IF NOT EXISTS taste_weights (
    facet TEXT NOT NULL CHECK (facet IN ('genre', 'category', 'developer', 'publisher', 'tag')),
    value TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 0,
    positive_samples INTEGER NOT NULL DEFAULT 0 CHECK (positive_samples >= 0),
    negative_samples INTEGER NOT NULL DEFAULT 0 CHECK (negative_samples >= 0),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (facet, value)
);

CREATE INDEX IF NOT EXISTS idx_taste_weights_ranked
    ON taste_weights(facet, weight DESC, value);

CREATE TABLE IF NOT EXISTS taste_feedback (
    id TEXT PRIMARY KEY,
    app_id INTEGER REFERENCES games(app_id) ON DELETE CASCADE,
    verdict TEXT NOT NULL CHECK (verdict IN ('interested', 'not_interested', 'owned_already')),
    surface TEXT NOT NULL DEFAULT 'upcoming',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_taste_feedback_recent
    ON taste_feedback(created_at DESC, app_id);

-- Próximos lanzamientos candidatos, puntuados contra el modelo de gustos.
CREATE TABLE IF NOT EXISTS upcoming_releases (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    title TEXT NOT NULL,
    capsule_url TEXT,
    header_url TEXT,
    release_date TEXT,
    release_date_is_exact INTEGER NOT NULL DEFAULT 0
        CHECK (release_date_is_exact IN (0, 1)),
    genres_json TEXT NOT NULL DEFAULT '[]',
    categories_json TEXT NOT NULL DEFAULT '[]',
    developer TEXT,
    publisher TEXT,
    short_description TEXT,
    match_score REAL NOT NULL DEFAULT 0,
    match_reason TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'store'
        CHECK (source IN ('store', 'library_relation', 'manual')),
    dismissed_at TEXT,
    discovered_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_upcoming_releases_ranked
    ON upcoming_releases(dismissed_at, match_score DESC, release_date ASC, app_id);
