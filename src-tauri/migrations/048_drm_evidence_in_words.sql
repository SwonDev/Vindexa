-- La evidencia del DRM decía el nombre interno de dos campos de la API.
--
-- # El fallo que arregla
--
-- Bajo la insignia «Sin DRM», la ficha explicaba por qué con esta frase:
--
--     La ficha oficial no declara drm_notice ni ext_user_account_notice.
--
-- `drm_notice` y `ext_user_account_notice` son los nombres que Steam le da a
-- dos campos de su respuesta. Quien lee la ficha no tiene por qué saberlo, y
-- una marca de DRM sólo vale si se puede comprobar: no se comprueba lo que no
-- se entiende. Es el mismo fallo que ya se corrigió en el rótulo de la fuente
-- —«extUserAccountNotice» pasó a «Cuenta externa que pide la ficha»—, que se
-- quedó sin corregir en el texto de la evidencia.
--
-- # Por qué una migración y no sólo cambiar la constante
--
-- La frase se guarda dentro de `drm_evidence_json` en el momento de clasificar,
-- así que cambiar el texto en el código sólo arregla los juegos que se
-- clasifiquen a partir de ahora. En una instalación real había 3.377 fichas con
-- la frase vieja guardada, y volver a preguntar por todas serían 3.377
-- peticiones a la tienda para reescribir una frase que ya está decidida.
--
-- La sustitución es literal y sobre el mismo texto, así que es idempotente:
-- aplicarla dos veces deja lo mismo que aplicarla una.
UPDATE games
   SET drm_evidence_json = REPLACE(
           drm_evidence_json,
           'La ficha oficial no declara drm_notice ni ext_user_account_notice.',
           'La ficha oficial no declara ningún aviso de DRM ni exige una cuenta externa.'
       )
 WHERE drm_evidence_json LIKE '%drm_notice ni ext_user_account_notice%';
