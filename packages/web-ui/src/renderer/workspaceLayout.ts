import type {
  ShellLayoutNode,
  ShellRegionPresentation,
  WebExperiencePack,
} from "../experience/types";

export interface WorkspacePreferences {
  placements: Record<string, string>;
  activeByRegion: Record<string, string>;
  collapsed: Record<string, boolean>;
  splitWeights: Record<string, number[]>;
}

export const EMPTY_WORKSPACE_PREFERENCES: WorkspacePreferences = {
  placements: {},
  activeByRegion: {},
  collapsed: {},
  splitWeights: {},
};

export function workspaceStorageKey(pack: WebExperiencePack): string {
  return `rintawa.web.workspace.v2:${pack.id}`;
}

function legacyWorkspaceStorageKey(pack: WebExperiencePack): string {
  return `rintawa.web.workspace.v1:${pack.id}`;
}

export function readWorkspacePreferences(
  pack: WebExperiencePack,
): WorkspacePreferences {
  if (typeof window === "undefined") return EMPTY_WORKSPACE_PREFERENCES;

  try {
    const raw =
      window.localStorage.getItem(workspaceStorageKey(pack)) ??
      window.localStorage.getItem(legacyWorkspaceStorageKey(pack));
    if (!raw) return EMPTY_WORKSPACE_PREFERENCES;

    const parsed = JSON.parse(raw) as Partial<WorkspacePreferences>;
    return {
      placements: parsed.placements ?? {},
      activeByRegion: parsed.activeByRegion ?? {},
      collapsed: parsed.collapsed ?? {},
      splitWeights: parsed.splitWeights ?? {},
    };
  } catch {
    return EMPTY_WORKSPACE_PREFERENCES;
  }
}

export function writeWorkspacePreferences(
  pack: WebExperiencePack,
  preferences: WorkspacePreferences,
): void {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(
      workspaceStorageKey(pack),
      JSON.stringify(preferences),
    );
  } catch {
    // Workspace persistence is best-effort; the renderer must remain usable.
  }
}

export function shellLayoutIdentity(node: ShellLayoutNode): string {
  if (node.type === "region") return `region:${node.region}`;
  return `split:${node.axis}(${node.children.map(shellLayoutIdentity).join("|")})`;
}

export function shellRegionPresentation(
  pack: WebExperiencePack,
  node: ShellLayoutNode,
): ShellRegionPresentation | undefined {
  if (node.type !== "region") return undefined;
  return pack.shell.region_presentations?.find(
    (presentation) => presentation.region === node.region,
  );
}

export function shellResizeBounds(
  pack: WebExperiencePack,
  node: ShellLayoutNode,
): { minimum: number; maximum: number } {
  const resize = shellRegionPresentation(pack, node)?.resize;
  return {
    minimum: resize?.min_size ?? 96,
    maximum: resize?.max_size ?? Number.POSITIVE_INFINITY,
  };
}

export function shellLayoutLabel(
  pack: WebExperiencePack,
  node: ShellLayoutNode,
): string {
  if (node.type !== "region") return "workspace panel";
  return shellRegionPresentation(pack, node)?.label ?? node.region;
}
