import { createContext, useContext } from "react";
import type { ComponentType, ReactNode } from "react";
import {
  IconAlertTriangle,
  IconBox,
  IconCheck,
  IconDatabase,
  IconDots,
  IconDownload,
  IconPackage,
  IconPackages,
  IconPaperclip,
  IconPuzzle,
  IconSearch,
  IconSend,
  IconSettings,
  IconSparkles,
  IconTool,
  IconTrash,
  IconUpload,
  IconUser,
  IconUsers,
  IconWorld,
} from "@tabler/icons-react";

type IconComponent = ComponentType<{
  size?: number | string;
  stroke?: number | string;
  "aria-hidden"?: boolean;
}>;

const STANDARD_GLYPHS: Record<string, IconComponent> = {
  box: IconBox,
  check: IconCheck,
  database: IconDatabase,
  dots: IconDots,
  download: IconDownload,
  package: IconPackage,
  packages: IconPackages,
  paperclip: IconPaperclip,
  puzzle: IconPuzzle,
  search: IconSearch,
  send: IconSend,
  settings: IconSettings,
  sparkles: IconSparkles,
  tool: IconTool,
  trash: IconTrash,
  upload: IconUpload,
  user: IconUser,
  users: IconUsers,
  warning: IconAlertTriangle,
  world: IconWorld,
};

const STANDARD_SLOT_MAP: Record<string, string> = {
  "activity.extensions": "packages",
  "activity.management": "tool",
  "activity.extension": "puzzle",
  "activity.worlds": "world",
  "activity.characters": "users",
  "character.portrait": "user",
  "world.thumbnail": "world",
  "participant.local": "user",
  "participant.remote": "sparkles",
  "action.download": "download",
  "action.update": "upload",
  "action.remove": "trash",
  "action.settings": "settings",
  "action.search": "search",
  "action.more": "dots",
  "action.attach": "paperclip",
  "action.send": "send",
  "status.warning": "warning",
  "status.success": "check",
  "repository.generic": "database",
  "fallback.package": "package",
  "fallback.generic": "box",
};

const IconMappingContext = createContext<Readonly<Record<string, string>>>({});

export function InterfaceIconProvider({
  mapping,
  children,
}: {
  mapping: Readonly<Record<string, string>>;
  children: ReactNode;
}) {
  return (
    <IconMappingContext.Provider value={mapping}>
      {children}
    </IconMappingContext.Provider>
  );
}

export function InterfaceIcon({
  slot,
  size = 20,
}: {
  slot: string | null | undefined;
  size?: number;
}) {
  const mapping = useContext(IconMappingContext);
  const glyphId =
    (slot ? mapping[slot] : undefined) ??
    (slot ? STANDARD_SLOT_MAP[slot] : undefined) ??
    STANDARD_SLOT_MAP["fallback.generic"];
  const Icon =
    (glyphId ? STANDARD_GLYPHS[glyphId] : undefined) ?? STANDARD_GLYPHS.box;
  return <Icon size={size} stroke={1.8} aria-hidden />;
}
