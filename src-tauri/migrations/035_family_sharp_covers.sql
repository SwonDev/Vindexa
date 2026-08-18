-- Carátulas del catálogo de Family a resolución real.
--
-- La migración 034 hizo esto mismo con `games` y dejó fuera el catálogo de
-- Steam Family, que se alimenta por otro camino. El resultado se veía: en una
-- biblioteca con 1801 juegos prestados, los 1801 seguían apuntando a
-- `library_600x900.jpg` mientras los propios ya usaban la variante grande, así
-- que las mismas carátulas se veían peor en una pantalla que en la otra.
--
-- `library_600x900.jpg` no mide 600×900: mide 300×450. La que sí los mide es
-- `library_600x900_2x.jpg`. Y hay títulos que directamente no publican la
-- pequeña, que es por lo que algunas portadas no llegaban a cargar.
--
-- El `replace` sólo actúa sobre ese nombre de archivo exacto: una portada que
-- venga de otro sitio se queda como está. Si un juego no publica la variante
-- grande, la escalera de `art_cache` baja sola al siguiente peldaño, así que
-- apuntar alto no deja a nadie sin imagen.
UPDATE family_catalog_games
   SET cover_url = replace(cover_url, 'library_600x900.jpg', 'library_600x900_2x.jpg'),
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE cover_url LIKE '%library_600x900.jpg';

-- Las copias cacheadas se derivaron de la URL pequeña: al vaciarlas, la próxima
-- vez que se pidan se descargan de la grande. Sólo se borra la fila del índice;
-- los archivos sueltos los recoge el mantenimiento de la caché.
DELETE FROM image_cache WHERE variant LIKE 'cover%';
