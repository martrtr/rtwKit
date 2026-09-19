import type { UiPlacementHint } from "../bridge/types";

export const WEB_EXPERIENCE_PACK_FORMAT = "rintawa.web.experience-pack@1" as const;
export const WEB_EXPERIENCE_MODULE_FORMAT = "rintawa.web.experience-module@1" as const;

export interface ExperienceTheme {
  tokens: Record<string, string>;
  icons?: Record<string, string>;
}

export type ShellLayoutNode =
  | {
      type: "region";
      region: string;
      weight?: number;
    }
  | {
      type: "split";
      axis: "horizontal" | "vertical";
      children: ShellLayoutNode[];
    };

export interface ShellLayer {
  region: string;
  presentation: "status" | "dialog" | "overlay";
}

export interface ShellRuleMatch {
  semantic?: string;
  trait?: string;
  placement?: UiPlacementHint;
}

export interface ShellRegionResize {
  edge: "start" | "end";
  min_size?: number;
  max_size?: number;
}

export interface ShellRegionPresentation {
  region: string;
  label: string;
  mode: "plain" | "activity-tabs";
  accepts_activities?: boolean;
  collapsible?: boolean;
  resize?: ShellRegionResize;
}

export interface ShellActivityBar {
  presentation: "vertical-start" | "vertical-end" | "horizontal-top" | "hidden";
}

export interface ShellRule {
  match: ShellRuleMatch;
  region: string;
}

export interface ExperienceShell {
  workspace: ShellLayoutNode;
  layers: ShellLayer[];
  rules: ShellRule[];
  fallback_region: string;
  region_presentations?: ShellRegionPresentation[];
  activity_bar?: ShellActivityBar;
}

export interface WebExperiencePack {
  format: typeof WEB_EXPERIENCE_PACK_FORMAT;
  id: string;
  name: string;
  theme: ExperienceTheme;
  shell: ExperienceShell;
}

export interface ExperienceThemePatch {
  tokens?: Record<string, string>;
  icons?: Record<string, string>;
}

export interface ExperienceShellPatch {
  workspace?: ShellLayoutNode;
  layers?: ShellLayer[];
  rules?: ShellRule[];
  rules_mode?: "prepend" | "replace";
  fallback_region?: string;
  region_presentations?: ShellRegionPresentation[];
  activity_bar?: ShellActivityBar;
}

export interface WebExperiencePackPatch {
  format: typeof WEB_EXPERIENCE_PACK_FORMAT;
  id: string;
  name: string;
  extends: string;
  theme?: ExperienceThemePatch;
  shell?: ExperienceShellPatch;
}

export interface WebExperienceModule {
  format: typeof WEB_EXPERIENCE_MODULE_FORMAT;
  id: string;
  name: string;
  targets?: string[];
  theme?: ExperienceThemePatch;
  shell?: ExperienceShellPatch;
}
