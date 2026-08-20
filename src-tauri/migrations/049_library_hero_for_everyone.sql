-- El banner de la ficha se veía apagado en casi toda la biblioteca.
--
-- # El fallo que arregla
--
-- `games.hero_url` guarda el **fondo de la página de tienda**: la imagen que
-- Steam pinta detrás del texto de su página, casi siempre un degradado oscuro.
-- El arte de biblioteca —`library_hero.jpg`, 1920×620 y a todo color— viaja en
-- `library_hero_url`, y sólo lo tenían 161 de 3.877 juegos: los enriquecidos
-- después del 18 de agosto, que es cuando se empezó a guardar. Los otros 1.786
-- se habían enriquecido el 14 y el 15, así que la ficha sólo tenía el fondo
-- apagado que ofrecerle al caché de arte.
--
-- Y no era sólo que eligiera la imagen peor: **impedía encontrar la buena**. El
-- caché ordena los candidatos por calidad y sólo prueba los que superan lo que
-- la interfaz pidió; el fondo de tienda no es ninguno de los peldaños
-- conocidos, así que entraba como la mejor posible y cortaba la búsqueda.
--
-- # Por qué se puede derivar
--
-- La URL del arte de biblioteca se compone igual que las demás de ese juego, y
-- Rust ya la deriva así cuando la respuesta de la tienda no trae ninguna base
-- —`conventional_library_asset`—. Aquí se hace lo mismo para los que se
-- quedaron sin ella, sin gastar 1.786 peticiones.
--
-- Medido sobre diez juegos al azar de los que no la tenían: nueve publican
-- `library_hero.jpg`. El décimo no, y ahí no se pierde nada: el caché prueba en
-- orden y cae en el fondo de tienda, que es exactamente lo que se veía antes.
--
-- Sólo juegos de Steam: los de Epic, GOG e itch.io llevan un identificador que
-- se inventó Vindexa y su arte no está en esa CDN.
UPDATE games
   SET library_hero_url =
           'https://shared.steamstatic.com/store_item_assets/steam/apps/'
           || app_id
           || '/library_hero.jpg'
 WHERE library_hero_url IS NULL
   AND (external_store IS NULL OR external_store = '')
   AND app_id < 2000000000;
