import type { UiPlacementHint } from "../bridge/types";

export const WEB_EXPERIENCE_PACK_FORMAT = "rintawa.web.experience-pack@1" as const;

export interface ExperienceTheme {
  tokens: Record<string, string>;
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
  placement?: UiPlacementHint;
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
}

export interface WebExperiencePack {
  format: typeof WEB_EXPERIENCE_PACK_FORMAT;
  id: string;
  name: string;
  theme: ExperienceTheme;
  shell: ExperienceShell;
}

export interface WebExperiencePackPatch {
  format: typeof WEB_EXPERIENCE_PACK_FORMAT;
  id: string;
  name: string;
  extends: string;
  theme?: {
    tokens?: Record<string, string>;
  };
  shell?: {
    workspace?: ShellLayoutNode;
    layers?: ShellLayer[];
    rules?: ShellRule[];
    rules_mode?: "prepend" | "replace";
    fallback_region?: string;
  };
}
