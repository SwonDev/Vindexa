-- Detección y marcado de juegos DRM-Free.
-- `drm_state` se deriva exclusivamente de señales oficiales publicadas por la
-- tienda (campo `drm_notice`, categorías y `ext_user_account_notice`). Nunca se
-- infiere de terceros ni se muestra sobre la carátula: es un dato de ficha.
ALTER TABLE games ADD COLUMN drm_notice TEXT;
ALTER TABLE games ADD COLUMN drm_state TEXT NOT NULL DEFAULT 'unknown'
    CHECK (drm_state IN ('unknown', 'drm_free', 'third_party_drm', 'steam_drm'));
ALTER TABLE games ADD COLUMN drm_evidence_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE games ADD COLUMN drm_checked_at TEXT;

CREATE INDEX IF NOT EXISTS idx_games_drm_state
    ON games(drm_state, title COLLATE NOCASE, app_id);
