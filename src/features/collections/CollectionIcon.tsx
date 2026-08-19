/**
 * El icono con el que se reconoce una colección de un vistazo.
 *
 * Es el mismo componente en todas partes —barra lateral, pantalla de
 * colecciones, editor, listas de deseados— a propósito: cuando cada sitio
 * pintaba el suyo, cambiar el icono de una colección lo cambiaba en la pantalla
 * de colecciones y **no** en la barra lateral, que es donde más se mira.
 */

import {
  type Icon,
  IconAlien,
  IconBolt,
  IconBook,
  IconBookmark,
  IconBrain,
  IconBuildingCastle,
  IconCards,
  IconChess,
  IconClock,
  IconCompass,
  IconCrown,
  IconDeviceGamepad2,
  IconDice,
  IconEye,
  IconFlag,
  IconFlame,
  IconFolder,
  IconFolders,
  IconGhost,
  IconGift,
  IconHeart,
  IconHourglass,
  IconMap,
  IconMedal,
  IconMountain,
  IconMovie,
  IconMusic,
  IconPin,
  IconPlanet,
  IconPuzzle,
  IconRobot,
  IconRocket,
  IconShield,
  IconSkull,
  IconSparkles,
  IconStar,
  IconSword,
  IconTag,
  IconTarget,
  IconTrophy,
  IconUsers,
  IconWand,
} from "@tabler/icons-react";
import type { CSSProperties } from "react";

/**
 * Iconos entre los que se puede elegir.
 *
 * El orden importa: se pintan en una rejilla y están agrupados por sentido
 * —organizar, marcar, jugar, mundos, tiempo y gente— para que encontrar el que
 * se busca no obligue a recorrerlos todos.
 *
 * Los valores son estables y se guardan en la base: renombrar uno dejaría a las
 * colecciones que lo usaran sin icono. Añadir es seguro; quitar, no.
 */
export const collectionIconOptions = [
  // Organizar
  { value: "folder", label: "Carpeta", icon: IconFolder },
  { value: "folders", label: "Carpetas", icon: IconFolders },
  { value: "bookmark", label: "Marcador", icon: IconBookmark },
  { value: "tag", label: "Etiqueta", icon: IconTag },
  { value: "pin", label: "Chincheta", icon: IconPin },
  { value: "flag", label: "Bandera", icon: IconFlag },
  // Destacar
  { value: "star", label: "Estrella", icon: IconStar },
  { value: "heart", label: "Favoritos", icon: IconHeart },
  { value: "sparkles", label: "Destellos", icon: IconSparkles },
  { value: "flame", label: "Llama", icon: IconFlame },
  { value: "bolt", label: "Rayo", icon: IconBolt },
  { value: "gift", label: "Regalo", icon: IconGift },
  // Jugar
  { value: "gamepad", label: "Mando", icon: IconDeviceGamepad2 },
  { value: "trophy", label: "Trofeo", icon: IconTrophy },
  { value: "medal", label: "Medalla", icon: IconMedal },
  { value: "crown", label: "Corona", icon: IconCrown },
  { value: "target", label: "Diana", icon: IconTarget },
  { value: "puzzle", label: "Puzle", icon: IconPuzzle },
  { value: "dice", label: "Dados", icon: IconDice },
  { value: "cards", label: "Cartas", icon: IconCards },
  { value: "chess", label: "Ajedrez", icon: IconChess },
  // Mundos
  { value: "sword", label: "Espada", icon: IconSword },
  { value: "shield", label: "Escudo", icon: IconShield },
  { value: "castle", label: "Castillo", icon: IconBuildingCastle },
  { value: "wand", label: "Varita", icon: IconWand },
  { value: "skull", label: "Calavera", icon: IconSkull },
  { value: "ghost", label: "Fantasma", icon: IconGhost },
  { value: "alien", label: "Alien", icon: IconAlien },
  { value: "robot", label: "Robot", icon: IconRobot },
  { value: "rocket", label: "Cohete", icon: IconRocket },
  { value: "planet", label: "Planeta", icon: IconPlanet },
  { value: "mountain", label: "Montaña", icon: IconMountain },
  { value: "map", label: "Mapa", icon: IconMap },
  { value: "compass", label: "Brújula", icon: IconCompass },
  // Historias, tiempo y gente
  { value: "book", label: "Libro", icon: IconBook },
  { value: "movie", label: "Película", icon: IconMovie },
  { value: "music", label: "Música", icon: IconMusic },
  { value: "brain", label: "Cerebro", icon: IconBrain },
  { value: "eye", label: "Ojo", icon: IconEye },
  { value: "clock", label: "Reloj", icon: IconClock },
  { value: "hourglass", label: "Reloj de arena", icon: IconHourglass },
  { value: "users", label: "Grupo", icon: IconUsers },
] satisfies readonly { value: string; label: string; icon: Icon }[];

export function CollectionIcon({
  name,
  fallback,
  size,
  className,
  style,
}: {
  name: string;
  /** Qué pintar cuando la colección no tiene icono propio o guarda uno retirado. */
  fallback: "manual" | "smart";
  size?: number | undefined;
  className?: string | undefined;
  style?: CSSProperties | undefined;
}) {
  const option = collectionIconOptions.find((item) => item.value === name);
  const IconComponent = option?.icon ?? (fallback === "smart" ? IconSparkles : IconFolder);
  return (
    <IconComponent
      aria-hidden="true"
      {...(size === undefined ? {} : { size })}
      {...(className === undefined ? {} : { className })}
      {...(style === undefined ? {} : { style })}
    />
  );
}

/**
 * Rejilla para elegir icono.
 *
 * Cuarenta y dos opciones en una lista desplegable son cuarenta y dos renglones
 * que recorrer para encontrar una forma que se reconoce de un vistazo. En
 * rejilla se ven todas a la vez, que es como se elige un icono.
 *
 * El nombre de cada uno viaja en `aria-label` y en `title`: quien navega con
 * teclado o con lector de pantalla oye «Cohete», no «botón».
 */
export function CollectionIconPicker({
  value,
  onChange,
  color,
  label = "Icono",
  id,
}: {
  value: string;
  onChange: (icon: string) => void;
  /** Color de la colección, para que la elección se vea como quedará. */
  color?: string | undefined;
  label?: string | undefined;
  id?: string | undefined;
}) {
  return (
    <div className="icon-picker" role="radiogroup" aria-label={label} id={id}>
      {collectionIconOptions.map((option) => {
        const OptionIcon = option.icon;
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.label}
            title={option.label}
            data-selected={selected}
            onClick={() => onChange(option.value)}
          >
            <OptionIcon aria-hidden="true" {...(color ? { style: { color } } : {})} />
          </button>
        );
      })}
    </div>
  );
}
