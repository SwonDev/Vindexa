-- El recibo de deshacer de una acción de agente tiene columna propia.
-- Hasta ahora viajaba dentro de `affected_json`, que existe para enumerar los
-- juegos afectados: mezclar ambas cosas hacía ilegible la auditoría.
ALTER TABLE agent_audit_log ADD COLUMN receipt_json TEXT;

-- Procedencia explícita del emparejado de un juego externo. Antes se codificaba
-- en `match_confidence = 1.0`, una convención que había que conocer para leer
-- la tabla.
ALTER TABLE external_games ADD COLUMN match_source TEXT NOT NULL DEFAULT 'automatic'
    CHECK (match_source IN ('automatic', 'manual'));

UPDATE external_games SET match_source = 'manual' WHERE match_confidence >= 1.0;

CREATE INDEX IF NOT EXISTS idx_external_games_match_source
    ON external_games(store, match_source)
    WHERE match_source = 'manual';
