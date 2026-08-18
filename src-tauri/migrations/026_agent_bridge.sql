-- Puente para agentes externos (Hermes). Toda acción de un agente queda
-- registrada con su intención original, el resultado y un identificador de
-- deshacer, para que ninguna automatización modifique la biblioteca en silencio.
CREATE TABLE IF NOT EXISTS agent_clients (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'hermes' CHECK (kind IN ('hermes', 'generic')),
    token_hash TEXT NOT NULL,
    scopes_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_seen_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_clients_name
    ON agent_clients(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS agent_audit_log (
    id TEXT PRIMARY KEY,
    client_id TEXT REFERENCES agent_clients(id) ON DELETE SET NULL,
    intent TEXT NOT NULL,
    utterance TEXT NOT NULL DEFAULT '',
    arguments_json TEXT NOT NULL DEFAULT '{}',
    result TEXT NOT NULL DEFAULT 'pending'
        CHECK (result IN ('pending', 'applied', 'rejected', 'failed', 'undone')),
    affected_json TEXT NOT NULL DEFAULT '[]',
    undo_token TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_agent_audit_recent
    ON agent_audit_log(created_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_agent_audit_undo
    ON agent_audit_log(undo_token)
    WHERE undo_token IS NOT NULL;
