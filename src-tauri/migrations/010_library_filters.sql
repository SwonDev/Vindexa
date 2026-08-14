CREATE INDEX IF NOT EXISTS idx_games_early_access_filter
    ON games(is_early_access, app_id);
CREATE INDEX IF NOT EXISTS idx_games_deck_filter
    ON games(steam_deck_status, app_id)
    WHERE steam_deck_status IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_games_achievements_filter
    ON games(achievements_total, achievements_unlocked, app_id)
    WHERE achievements_total IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_personal_progress_filter
    ON game_personal(progress, app_id);
CREATE INDEX IF NOT EXISTS idx_personal_rating_filter
    ON game_personal(rating, app_id)
    WHERE rating IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_game_tags_tag_filter
    ON game_tags(tag_id, app_id);
