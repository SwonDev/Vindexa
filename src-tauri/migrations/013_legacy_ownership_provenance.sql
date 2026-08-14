-- La migración 007 tuvo que conservar su DEFAULT histórico `owned`, pero las
-- filas que ya existían entonces no contenían procedencia. Demotamos sólo esa
-- cohorte y sólo cuando no conserva ninguna señal exclusiva de GetOwnedGames.
-- Es deliberadamente conservador: una sincronización Web API posterior vuelve
-- a promover el AppID a `owned` mediante el upsert normal.
UPDATE games
   SET ownership_source = 'local',
       family_availability = 'not_applicable',
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE ownership_source = 'owned'
   AND EXISTS (
       SELECT 1
         FROM schema_migrations migration
        WHERE migration.version = 7
          AND games.imported_at <= migration.applied_at
   )
   AND playtime_minutes = 0
   AND playtime_recent_minutes = 0
   AND last_played_at IS NULL
   AND icon_url IS NULL;
