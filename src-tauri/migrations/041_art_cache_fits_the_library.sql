-- La caché de arte deja de tener techo.
--
-- Venía de fábrica con 512 MiB, y eso es menos de lo que ocupan las carátulas
-- de una biblioteca grande: alrededor de un giga para cuatro mil juegos, sin
-- contar banners ni cabeceras. Con el techo por debajo de lo que hace falta,
-- cada imagen nueva desalojaba una anterior y se acababa descargando lo mismo
-- una y otra vez.
--
-- Vindexa es una aplicación local: el arte vive en el disco de quien la usa, y
-- ponerle un número arbitrario sólo servía para que su propia biblioteca no
-- cupiera en su propio disco. Cero significa sin techo. El único límite que
-- queda es físico —no comerse el espacio libre que el sistema necesita— y lo
-- vigila la caché, no un ajuste.
--
-- Sólo se cambia el valor de fábrica. Quien fijara uno a mano mandó él, y su
-- decisión se respeta.
UPDATE app_settings
   SET value = '0',
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE key = 'art_cache_mib'
   AND value = '512';
