import {
  type Icon,
  IconBookmark,
  IconFolders,
  IconHeart,
  IconSparkles,
  IconTrophy,
  IconUsers,
} from "@tabler/icons-react";

export const collectionIconOptions = [
  { value: "folder", label: "Carpeta", icon: IconFolders },
  { value: "sparkles", label: "Destellos", icon: IconSparkles },
  { value: "heart", label: "Favoritos", icon: IconHeart },
  { value: "bookmark", label: "Marcador", icon: IconBookmark },
  { value: "trophy", label: "Trofeo", icon: IconTrophy },
  { value: "users", label: "Grupo", icon: IconUsers },
] satisfies readonly { value: string; label: string; icon: Icon }[];

export function CollectionIcon({ name, fallback }: { name: string; fallback: "manual" | "smart" }) {
  const option = collectionIconOptions.find((item) => item.value === name);
  const IconComponent = option?.icon ?? (fallback === "smart" ? IconSparkles : IconFolders);
  return <IconComponent aria-hidden="true" />;
}
