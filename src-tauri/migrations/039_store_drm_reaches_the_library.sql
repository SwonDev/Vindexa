-- Lo que la tienda sabe del DRM llega a la ficha del juego.
--
-- La migración 037 metió los juegos de Epic, GOG e itch.io en la biblioteca,
-- pero se dejó por el camino algo que su catálogo sí sabía: si el juego lleva
-- DRM. GOG lo declara para todo lo que vende, así que 44 juegos que constan
-- DRM-free en `external_games` aparecían como «sin dato» en su ficha.
--
-- # Sólo se rellena lo que no se sabía
--
-- Si la fila ya afirma algo, se conserva. Ese dato viene de los avisos oficiales
-- de la ficha de Steam —el módulo `steam::drm` los clasifica y guarda la frase
-- que lo motivó—, y una política general de tienda no puede pisar una evidencia
-- concreta del propio juego.
--
-- # La marca no se dibuja sobre la carátula
--
-- Es un requisito de producto, no una preferencia: el DRM es un dato de ficha y
-- se enseña dentro del detalle, siempre acompañado de la evidencia que lo
-- justifica. Ver la cabecera de `src/steam/drm.rs`.
UPDATE games
   SET drm_state = 'drm_free',
       drm_evidence_json = json_array(
           json_object('source', 'storeCatalogue', 'matched', 'catálogo GOG')
       ),
       drm_checked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE drm_state = 'unknown'
   AND EXISTS (
        SELECT 1 FROM external_games e
         WHERE e.local_app_id = games.app_id
           AND e.store = 'gog'
           AND e.drm_state = 'drm_free'
   );
