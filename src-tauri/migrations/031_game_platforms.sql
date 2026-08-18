-- Plataformas en las que el juego se puede instalar, según la tienda.
--
-- Las tres columnas admiten NULL a propósito: **desconocido no es lo mismo que
-- no compatible**. Un juego cuya ficha aún no se ha enriquecido no debe
-- presentarse como incompatible con el sistema de la persona, ni al revés. La
-- interfaz sólo puede desaconsejar una instalación cuando tiene el dato.
ALTER TABLE games ADD COLUMN platform_windows INTEGER;
ALTER TABLE games ADD COLUMN platform_mac INTEGER;
ALTER TABLE games ADD COLUMN platform_linux INTEGER;

-- Filtrar «lo que puedo instalar aquí» es una consulta de biblioteca, así que
-- el índice cubre el caso de este equipo sin penalizar al resto.
CREATE INDEX IF NOT EXISTS idx_games_platform_mac
    ON games(platform_mac)
    WHERE platform_mac IS NOT NULL;
