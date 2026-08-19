-- Un juego comprado en otra tienda es tuyo, aunque en Steam sólo lo tengas
-- prestado.
--
-- La migración 037 vinculó cada juego de Epic, GOG e itch.io con su fila de la
-- biblioteca, y cuando había equivalente en Steam usó esa. El problema aparece
-- cuando ese equivalente venía del préstamo familiar y sin confirmar: la
-- biblioteca oculta esas filas —tenerlas a la vista no es tenerlas— y con ellas
-- se escondieron juegos que **sí** se habían comprado.
--
-- No era poco: en una biblioteca real desaparecieron de la vista 158 juegos
-- comprados en Epic. La tarjeta de Ajustes decía 554 y el ámbito de la
-- biblioteca 395, y la diferencia no tenía ninguna explicación visible.
--
-- La propiedad gana al préstamo: si consta una compra, la fila pasa a `owned`.
-- Que además esté prestado en Steam deja de ser relevante para decidir si se
-- enseña, y el catálogo del préstamo sigue guardando ese hecho en
-- `family_catalog_games`.
UPDATE games
   SET ownership_source = 'owned',
       family_availability = 'not_applicable',
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE ownership_source = 'family_shared'
   AND EXISTS (
        SELECT 1 FROM external_games e WHERE e.local_app_id = games.app_id
   );
