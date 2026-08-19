-- Cuándo se miró por última vez cada deseado buscando su fecha de salida.
--
-- Los próximos lanzamientos se buscan por tandas, porque cada uno cuesta una
-- petición a la tienda y una lista de novecientos no se revisa de una sentada
-- sin que Steam frene. Para que las tandas avancen hace falta saber por dónde
-- se iba.
--
-- No vale con mirar `upcoming_releases.updated_at`: ahí sólo están los que
-- **sí** están por salir. Un deseado que ya se publicó no deja rastro allí, así
-- que volvería a encabezar la cola en cada pasada y las siguientes nunca
-- llegarían al final de la lista.
CREATE TABLE IF NOT EXISTS upcoming_checks (
    app_id INTEGER PRIMARY KEY,
    checked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
