CREATE INDEX IF NOT EXISTS idx_tags_name_lookup
    ON tags(name COLLATE NOCASE ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_game_tags_tag_game
    ON game_tags(tag_id, app_id);

CREATE INDEX IF NOT EXISTS idx_sessions_finished
    ON game_sessions(app_id, ended_at DESC)
    WHERE ended_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_personal_milestones
    ON game_personal(started_at, completed_at, abandoned_at, app_id);

CREATE TRIGGER IF NOT EXISTS tags_validate_insert
BEFORE INSERT ON tags
WHEN length(trim(new.name)) NOT BETWEEN 1 AND 40
  OR length(new.color) <> 7
  OR substr(new.color, 1, 1) <> '#'
  OR substr(new.color, 2) GLOB '*[^0-9A-Fa-f]*'
BEGIN
    SELECT RAISE(ABORT, 'invalid personal tag');
END;

CREATE TRIGGER IF NOT EXISTS tags_validate_update
BEFORE UPDATE OF name, color ON tags
WHEN length(trim(new.name)) NOT BETWEEN 1 AND 40
  OR length(new.color) <> 7
  OR substr(new.color, 1, 1) <> '#'
  OR substr(new.color, 2) GLOB '*[^0-9A-Fa-f]*'
BEGIN
    SELECT RAISE(ABORT, 'invalid personal tag');
END;

CREATE TRIGGER IF NOT EXISTS sessions_validate_insert
BEFORE INSERT ON game_sessions
WHEN datetime(new.started_at) IS NULL
  OR (new.ended_at IS NOT NULL AND datetime(new.ended_at) IS NULL)
  OR (new.ended_at IS NOT NULL AND datetime(new.ended_at) < datetime(new.started_at))
  OR length(new.note) > 2000
BEGIN
    SELECT RAISE(ABORT, 'invalid game session');
END;

CREATE TRIGGER IF NOT EXISTS sessions_validate_update
BEFORE UPDATE OF started_at, ended_at, progress_before, progress_after, note ON game_sessions
WHEN datetime(new.started_at) IS NULL
  OR (new.ended_at IS NOT NULL AND datetime(new.ended_at) IS NULL)
  OR (new.ended_at IS NOT NULL AND datetime(new.ended_at) < datetime(new.started_at))
  OR length(new.note) > 2000
BEGIN
    SELECT RAISE(ABORT, 'invalid game session');
END;

CREATE TRIGGER IF NOT EXISTS personal_dates_validate_update
BEFORE UPDATE OF started_at, completed_at, abandoned_at ON game_personal
WHEN (new.started_at IS NOT NULL AND date(new.started_at) IS NULL)
  OR (new.completed_at IS NOT NULL AND date(new.completed_at) IS NULL)
  OR (new.abandoned_at IS NOT NULL AND date(new.abandoned_at) IS NULL)
  OR (new.completed_at IS NOT NULL AND new.abandoned_at IS NOT NULL)
  OR (new.started_at IS NOT NULL AND new.completed_at IS NOT NULL
      AND date(new.completed_at) < date(new.started_at))
  OR (new.started_at IS NOT NULL AND new.abandoned_at IS NOT NULL
      AND date(new.abandoned_at) < date(new.started_at))
BEGIN
    SELECT RAISE(ABORT, 'invalid personal dates');
END;
