-- «Aún no ha salido» es una respuesta, y se estaba leyendo como «retirado».
--
-- # El fallo que arregla
--
-- La lista de deseados decía «no a la venta · gratuito, retirado o sin
-- publicar» en 354 juegos que **todavía no se han publicado**. Medido contra la
-- tienda el 20/08/2026: de diez tomados al azar entre los que no tenían precio,
-- los diez venían con `is_coming_soon`.
--
-- La pantalla sí sabía decir «aún no ha salido», pero sólo para los que
-- estuvieran en `upcoming_releases`, que es la lista **curada** de candidatos
-- puntuados por el modelo de gustos —112 filas—, no un registro de qué se ha
-- publicado y qué no. De los 453 sin precio, sólo 99 caían ahí.
--
-- # Qué cambia
--
-- `game_price_checks.outcome` admite una tercera respuesta. Las tres son
-- distintas y ninguna se puede deducir de las otras:
--
-- * `no_price`     la tienda respondió y no publica precio (gratuito, retirado);
-- * `unavailable`  la tienda no reconoce el AppID;
-- * `coming_soon`  la tienda respondió y el juego aún no se ha publicado.
--
-- Las filas existentes se conservan tal cual: `no_price` sigue siendo verdad
-- —la tienda no publicó precio— y la siguiente pasada las reclasificará con lo
-- que diga el índice.
--
-- SQLite no permite ampliar un `CHECK`, así que la tabla se reconstruye. Son
-- cuatrocientas filas.
CREATE TABLE game_price_checks_nueva (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    checked_at TEXT NOT NULL,
    -- `no_price`: la tienda respondió por el juego y no trae bloque de precio.
    -- `unavailable`: la tienda no reconoció el AppID (retirado o regional).
    -- `coming_soon`: la tienda respondió y el juego aún no se ha publicado.
    outcome TEXT NOT NULL CHECK (outcome IN ('no_price', 'unavailable', 'coming_soon'))
);

INSERT INTO game_price_checks_nueva(app_id, checked_at, outcome)
SELECT app_id, checked_at, outcome FROM game_price_checks;

DROP TABLE game_price_checks;
ALTER TABLE game_price_checks_nueva RENAME TO game_price_checks;
