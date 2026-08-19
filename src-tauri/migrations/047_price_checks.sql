-- «Sin precio consultado» y «sin precio publicado» no son lo mismo.
--
-- # El fallo que arregla
--
-- La pantalla de deseados decía «451 juegos sin precio consultado». Se habían
-- consultado los 451: la tienda respondió por ellos y **no publica precio**
-- —sin fecha de salida, gratuitos o retirados—. Sin guardar esa respuesta, un
-- juego preguntado y sin precio es indistinguible de uno al que nadie ha
-- preguntado nunca, y la frase acusaba a la aplicación de no haber mirado.
--
-- # Qué se guarda
--
-- La respuesta, no el precio: cuándo se preguntó y qué contestó la tienda. Con
-- eso la pantalla puede decir la verdad y la cola puede dejar de reintentar
-- cada seis horas algo que no va a cambiar en semanas.
CREATE TABLE IF NOT EXISTS game_price_checks (
    app_id INTEGER PRIMARY KEY CHECK (app_id > 0),
    checked_at TEXT NOT NULL,
    -- `no_price`: la tienda respondió por el juego y no trae bloque de precio.
    -- `unavailable`: la tienda no reconoció el AppID (retirado o regional).
    outcome TEXT NOT NULL CHECK (outcome IN ('no_price', 'unavailable'))
);

CREATE INDEX IF NOT EXISTS idx_game_price_checks_checked
    ON game_price_checks(checked_at);
