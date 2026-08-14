CREATE INDEX IF NOT EXISTS idx_games_title ON games(title COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_games_last_played ON games(last_played_at DESC);
CREATE INDEX IF NOT EXISTS idx_games_playtime ON games(playtime_minutes DESC);
CREATE INDEX IF NOT EXISTS idx_personal_status_position ON game_personal(status_id, manual_position);
CREATE INDEX IF NOT EXISTS idx_personal_installed ON game_personal(installed) WHERE installed = 1;
CREATE INDEX IF NOT EXISTS idx_personal_tracking ON game_personal(tracking) WHERE tracking = 1;
CREATE INDEX IF NOT EXISTS idx_personal_target ON game_personal(target_date) WHERE target_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_collection_games_position ON collection_games(collection_id, position);
CREATE INDEX IF NOT EXISTS idx_smart_rules_collection ON smart_rules(collection_id, group_id, position);
CREATE INDEX IF NOT EXISTS idx_planner_items_position ON planner_items(column_id, position);
CREATE INDEX IF NOT EXISTS idx_sessions_game_started ON game_sessions(app_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_activity_created ON activity(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sync_runs_started ON sync_runs(started_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS game_search USING fts5(
    title,
    notes,
    checkpoint,
    next_action
);

CREATE TRIGGER IF NOT EXISTS games_search_insert AFTER INSERT ON games BEGIN
    INSERT INTO game_search(rowid, title, notes, checkpoint, next_action)
    VALUES (new.app_id, new.title, '', '', '');
END;

CREATE TRIGGER IF NOT EXISTS games_search_title_update AFTER UPDATE OF title ON games BEGIN
    DELETE FROM game_search WHERE rowid = old.app_id;
    INSERT INTO game_search(rowid, title, notes, checkpoint, next_action)
    SELECT new.app_id, new.title, COALESCE(p.notes, ''), COALESCE(p.checkpoint, ''), COALESCE(p.next_action, '')
    FROM game_personal p WHERE p.app_id = new.app_id;
END;

CREATE TRIGGER IF NOT EXISTS personal_search_update AFTER UPDATE OF notes, checkpoint, next_action ON game_personal BEGIN
    DELETE FROM game_search WHERE rowid = old.app_id;
    INSERT INTO game_search(rowid, title, notes, checkpoint, next_action)
    SELECT g.app_id, g.title, COALESCE(new.notes, ''), COALESCE(new.checkpoint, ''), COALESCE(new.next_action, '')
    FROM games g WHERE g.app_id = new.app_id;
END;
