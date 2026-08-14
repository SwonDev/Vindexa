CREATE INDEX IF NOT EXISTS idx_games_imported_sort
    ON games(imported_at DESC, title COLLATE NOCASE, app_id);
CREATE INDEX IF NOT EXISTS idx_games_release_sort
    ON games(release_date DESC, title COLLATE NOCASE, app_id)
    WHERE release_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_personal_manual_sort
    ON game_personal(pinned DESC, priority DESC, manual_position, app_id);
CREATE INDEX IF NOT EXISTS idx_personal_installed_sort
    ON game_personal(installed DESC, app_id);
CREATE INDEX IF NOT EXISTS idx_installations_size_sort
    ON game_installations(app_id, is_primary, size_on_disk DESC);
