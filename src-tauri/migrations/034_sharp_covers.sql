-- Carátulas verticales a resolución real.
--
-- `library_600x900.jpg` no mide 600×900: mide **300×450**, comprobado contra la
-- CDN de Steam. La que sí tiene 600×900 es `library_600x900_2x.jpg`. En una
-- pantalla de densidad doble, una carátula de 208 px lógicos necesita 416 px
-- reales, de modo que la de 300 se ampliaba y se veía borrosa.
--
-- Las filas ya guardadas apuntan a la pequeña, así que se reescriben. No se
-- toca ninguna otra columna, y los juegos cuya carátula viene de otro sitio
-- —o que no tienen— quedan como estaban: el `replace` sólo actúa sobre el
-- nombre de archivo exacto.
--
-- Si un juego no publica la variante grande, la escalera de `art_cache` baja
-- sola al siguiente peldaño, así que apuntar alto no deja a nadie sin imagen.
UPDATE games
   SET cover_url = replace(cover_url, 'library_600x900.jpg', 'library_600x900_2x.jpg'),
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE cover_url LIKE '%library_600x900.jpg';

-- Las copias cacheadas se derivaron de la URL pequeña: al vaciarlas, la próxima
-- vez que se pidan se descargan de la grande. Sólo se borra la fila del índice;
-- los archivos sueltos los recoge el mantenimiento de la caché.
DELETE FROM image_cache WHERE variant LIKE 'cover%';
