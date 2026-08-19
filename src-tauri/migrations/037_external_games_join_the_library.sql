-- Los juegos de Epic, GOG e itch.io entran en la biblioteca.
--
-- Hasta ahora vivían sólo en `external_games`, una tabla de catálogo. Como
-- `game_personal` cuelga de `games`, no había dónde guardarles un estado, una
-- colección, una prioridad ni una nota: se podían mirar y nada más. Ni
-- arrastrar, ni clasificar, ni planificar. Es exactamente el mismo agujero que
-- la migración 036 cerró para el préstamo familiar, y se cierra igual.
--
-- # Por qué entran como propios
--
-- Porque lo son: se compraron. A diferencia del préstamo familiar —que se mira
-- pero no se tiene—, un juego de GOG es tan tuyo como uno de Steam, así que
-- cuenta en «Todos los juegos» y se organiza sin distinción.
--
-- # Los que ya estaban
--
-- Un juego externo emparejado con uno de Steam **no** se duplica: apunta a la
-- fila que ya existe. Por eso `local_app_id` es `matched_app_id` cuando lo hay.
-- Así, tener el mismo juego en Steam y en GOG sigue siendo un juego, no dos, y
-- el ámbito de GOG lo enseña igual porque el vínculo existe en las dos
-- direcciones.
--
-- # De dónde salen los identificadores
--
-- `games` se indexa por AppID de Steam, y un juego que no está en Steam no
-- tiene ninguno. Se le asigna uno local a partir de 2.000.000.000: Steam anda
-- por los siete millones largos y numera de forma creciente, así que ese tramo
-- no lo alcanzará. Se guarda en `external_games.local_app_id` para que el
-- vínculo sea explícito y comprobable, en vez de recalcularse por convención
-- cada vez.

-- Qué tienda trae cada juego. `NULL` es Steam, que es de donde viene la
-- inmensa mayoría y no necesita marca.
ALTER TABLE games ADD COLUMN external_store TEXT;

-- A qué fila de `games` corresponde cada juego externo.
ALTER TABLE external_games ADD COLUMN local_app_id INTEGER;

CREATE INDEX IF NOT EXISTS idx_games_external_store
    ON games(external_store) WHERE external_store IS NOT NULL;
-- El índice **no** es único a propósito: una misma obra puede aparecer varias
-- veces en la misma tienda —una edición base y otra especial, por ejemplo— y
-- las dos emparejan con el mismo juego de Steam. Comprobado en una biblioteca
-- real. Quien identifica a un juego externo sigue siendo `(store, external_id)`, que
-- ya es la clave primaria de la tabla.
CREATE INDEX IF NOT EXISTS idx_external_games_local_app_id
    ON external_games(local_app_id) WHERE local_app_id IS NOT NULL;

-- 1. El que ya tiene equivalente en Steam apunta a esa fila.
UPDATE external_games
   SET local_app_id = matched_app_id
 WHERE matched_app_id IS NOT NULL
   AND EXISTS (SELECT 1 FROM games g WHERE g.app_id = external_games.matched_app_id);

-- 2. El resto recibe un identificador local. El orden es estable —tienda y
--    luego identificador de la tienda— para que dos equipos con la misma
--    biblioteca lleguen al mismo reparto.
WITH pendientes AS (
    SELECT store,
           external_id,
           2000000000 + ROW_NUMBER() OVER (ORDER BY store, external_id) AS asignado
      FROM external_games
     WHERE local_app_id IS NULL
)
UPDATE external_games
   SET local_app_id = (
        SELECT p.asignado
          FROM pendientes p
         WHERE p.store = external_games.store
           AND p.external_id = external_games.external_id
       )
 WHERE local_app_id IS NULL;

-- 3. Los que no estaban en la biblioteca entran ahora.
INSERT INTO games (
    app_id, title, cover_url, header_url,
    playtime_minutes, playtime_recent_minutes,
    ownership_source, family_availability, external_store
)
SELECT e.local_app_id, e.title, e.cover_url, e.header_url,
       0, 0, 'owned', 'not_applicable', e.store
  FROM external_games e
 WHERE e.local_app_id IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM games g WHERE g.app_id = e.local_app_id);

-- 4. Sin ficha personal no hay estado ni orden manual, y la biblioteca —que une
--    `games` con `game_personal`— no los devolvería.
INSERT INTO game_personal (app_id, status_id)
SELECT g.app_id, 'unclassified'
  FROM games g
 WHERE g.external_store IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM game_personal p WHERE p.app_id = g.app_id);
