-- Los juegos del préstamo familiar entran en la biblioteca para poder
-- organizarse.
--
-- Hasta ahora vivían sólo en `family_catalog_games`, una tabla de catálogo sin
-- ficha personal. Como `game_personal` cuelga de `games`, no había dónde
-- guardarles un estado, una colección, una prioridad ni una nota: se podían
-- mirar y nada más. Eso no era una decisión, era una consecuencia de haber
-- modelado el catálogo como un listado de sólo lectura.
--
-- No es una promoción a «tuyos». Entran con `ownership_source = 'family_shared'`
-- y conservan su `family_availability`, y tanto `list_games` como
-- `library_stats` ya excluían esas filas de la biblioteca propia salvo que se
-- pidan a propósito: el modelo estaba preparado para esto desde la migración
-- 007, sólo faltaba completarlo.
--
-- Se conserva `family_catalog_games` como tabla de importación: es donde escribe
-- la sincronización antes de volcar aquí.
INSERT INTO games (
    app_id, title, icon_url, cover_url, header_url,
    playtime_minutes, playtime_recent_minutes,
    ownership_source, family_availability
)
SELECT f.app_id, f.title, f.icon_url, f.cover_url, f.header_url,
       0, 0, 'family_shared', f.availability
  FROM family_catalog_games f
 WHERE NOT EXISTS (SELECT 1 FROM games g WHERE g.app_id = f.app_id);

-- Sin ficha personal no hay estado ni orden manual, así que la biblioteca no
-- los devolvería: la consulta une `games` con `game_personal`.
INSERT INTO game_personal (app_id, status_id)
SELECT g.app_id, 'unclassified'
  FROM games g
 WHERE g.ownership_source = 'family_shared'
   AND NOT EXISTS (SELECT 1 FROM game_personal p WHERE p.app_id = g.app_id);
