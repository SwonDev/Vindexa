ALTER TABLE planner_items ADD COLUMN queue_position INTEGER;
ALTER TABLE planner_items ADD COLUMN planned_for TEXT;
ALTER TABLE planner_items ADD COLUMN objective TEXT NOT NULL DEFAULT '' CHECK (length(objective) <= 160);

WITH ranked AS (
    SELECT pi.app_id,
           ROW_NUMBER() OVER (
               ORDER BY pc.position ASC, pi.position ASC, pi.app_id ASC
           ) - 1 AS queue_position
      FROM planner_items pi
      JOIN planner_columns pc ON pc.id = pi.column_id
)
UPDATE planner_items
   SET queue_position = (
       SELECT ranked.queue_position
         FROM ranked
        WHERE ranked.app_id = planner_items.app_id
   );

CREATE UNIQUE INDEX IF NOT EXISTS idx_planner_items_queue_position
    ON planner_items(queue_position)
    WHERE queue_position IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_planner_items_planned_for
    ON planner_items(planned_for, queue_position);

CREATE TABLE IF NOT EXISTS planner_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    weekly_capacity_minutes INTEGER NOT NULL DEFAULT 600
        CHECK (weekly_capacity_minutes BETWEEN 60 AND 600000),
    monthly_capacity_minutes INTEGER NOT NULL DEFAULT 2400
        CHECK (monthly_capacity_minutes BETWEEN 60 AND 2400000),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT OR IGNORE INTO planner_settings(
    id, weekly_capacity_minutes, monthly_capacity_minutes
) VALUES (1, 600, 2400);
