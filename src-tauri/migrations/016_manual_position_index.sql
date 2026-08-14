DROP INDEX IF EXISTS idx_personal_manual_sort;
CREATE INDEX idx_personal_manual_sort
    ON game_personal(manual_position ASC, pinned DESC, priority DESC, app_id ASC);
