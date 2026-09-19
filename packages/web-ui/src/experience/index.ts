import standardPackJson from "./standard.json";

import type { UiPresentationSurface } from "../bridge";
import {
  parseExperienceModule,
  parseExperiencePack,
  parseExperiencePackPatch,
  validateExperiencePack,
} from "./schema";
import type {
  ExperienceShellPatch,
  ExperienceThemePatch,
  WebExperiencePack,
} from "./types";

export type {
  ShellLayoutNode,
  WebExperiencePack,
  WebExperienceModule,
  WebExperiencePackPatch,
} from "./types";
export { WEB_EXPERIENCE_MODULE_FORMAT, WEB_EXPERIENCE_PACK_FORMAT } from "./types";
export { parseExperienceModule, parseExperiencePack, parseExperiencePackPatch } from "./schema";

export const STANDARD_EXPERIENCE_PACK = parseExperiencePack(standardPackJson);

function mergePresentation(
  base: WebExperiencePack,
  themePatch: ExperienceThemePatch | undefined,
  shellPatch: ExperienceShellPatch | undefined,
  identity: { id: string; name: string },
): WebExperiencePack {
  const patchRules = shellPatch?.rules;
  const rules =
    patchRules === undefined
      ? base.shell.rules
      : shellPatch?.rules_mode === "replace"
        ? patchRules
        : [...patchRules, ...base.shell.rules];

  const replacesWorkspace = shellPatch?.workspace !== undefined;

  return validateExperiencePack({
    format: base.format,
    id: identity.id,
    name: identity.name,
    theme: {
      tokens: {
        ...base.theme.tokens,
        ...(themePatch?.tokens ?? {}),
      },
      icons: {
        ...(base.theme.icons ?? {}),
        ...(themePatch?.icons ?? {}),
      },
    },
    shell: {
      workspace: shellPatch?.workspace ?? base.shell.workspace,
      layers: shellPatch?.layers ?? base.shell.layers,
      rules,
      fallback_region: shellPatch?.fallback_region ?? base.shell.fallback_region,
      region_presentations:
        shellPatch?.region_presentations ??
        (replacesWorkspace ? undefined : base.shell.region_presentations),
      activity_bar:
        shellPatch?.activity_bar ??
        (replacesWorkspace ? undefined : base.shell.activity_bar),
    },
  });
}

export function mergeExperiencePack(
  base: WebExperiencePack,
  patchValue: unknown,
): WebExperiencePack {
  const patch = parseExperiencePackPatch(patchValue);
  if (patch.extends !== base.id) {
    throw new Error(
      `experience pack '${patch.id}' extends '${patch.extends}', expected '${base.id}'`,
    );
  }

  return mergePresentation(base, patch.theme, patch.shell, {
    id: patch.id,
    name: patch.name,
  });
}

export function applyExperienceModule(
  base: WebExperiencePack,
  moduleValue: unknown,
): WebExperiencePack {
  const module = parseExperienceModule(moduleValue);
  if (module.targets !== undefined && !module.targets.includes(base.id)) {
    throw new Error(
      `experience module '${module.id}' does not target base pack '${base.id}'`,
    );
  }
  return mergePresentation(base, module.theme, module.shell, {
    id: base.id,
    name: base.name,
  });
}

export function resolveExperiencePack(value: unknown): WebExperiencePack {
  if (value === undefined || value === null) {
    return STANDARD_EXPERIENCE_PACK;
  }
  if (typeof value === "object" && value !== null && "extends" in value) {
    return mergeExperiencePack(STANDARD_EXPERIENCE_PACK, value);
  }
  return parseExperiencePack(value);
}

export function regionForSurface(
  pack: WebExperiencePack,
  surface: UiPresentationSurface,
): string {
  const semantic = surface.contribution.semantic
    ? `${surface.contribution.semantic.id}@${surface.contribution.semantic.version}`
    : null;

  let selected: { region: string; specificity: number } | null = null;
  for (const rule of pack.shell.rules) {
    if (rule.match.semantic !== undefined && rule.match.semantic !== semantic) continue;
    if (
      rule.match.trait !== undefined &&
      !surface.contribution.traits.includes(rule.match.trait)
    ) {
      continue;
    }
    if (
      rule.match.placement !== undefined &&
      rule.match.placement !== surface.contribution.placement
    ) {
      continue;
    }

    const specificity =
      (rule.match.semantic !== undefined ? 4 : 0) +
      (rule.match.trait !== undefined ? 2 : 0) +
      (rule.match.placement !== undefined ? 1 : 0);
    if (selected === null || specificity > selected.specificity) {
      selected = { region: rule.region, specificity };
    }
  }

  return selected?.region ?? pack.shell.fallback_region;
}

const TOKEN_TO_CSS_VARIABLE: Record<string, string> = {
  "ui.color.text.primary": "--rintawa-ui-color-text-primary",
  "ui.color.text.muted": "--rintawa-ui-color-text-muted",
  "ui.color.surface.primary": "--rintawa-ui-color-surface-primary",
  "ui.color.surface.secondary": "--rintawa-ui-color-surface-secondary",
  "ui.color.accent": "--rintawa-ui-color-accent",
  "ui.color.warning": "--rintawa-ui-color-warning",
  "ui.color.error": "--rintawa-ui-color-error",
  "ui.color.success": "--rintawa-ui-color-success",
  "ui.spacing.compact": "--rintawa-ui-spacing-compact",
  "ui.spacing.normal": "--rintawa-ui-spacing-normal",
  "ui.spacing.relaxed": "--rintawa-ui-spacing-relaxed",
  "ui.radius.control": "--rintawa-ui-radius-control",
  "ui.font.body": "--rintawa-ui-font-body",
  "ui.font.heading": "--rintawa-ui-font-heading",
  "ui.font.monospace": "--rintawa-ui-font-monospace",
  "web.color.background": "--rintawa-web-color-background",
  "web.color.border": "--rintawa-web-color-border",
  "web.color.control-border": "--rintawa-web-color-control-border",
  "web.radius.surface": "--rintawa-web-radius-surface",
  "web.workbench.activity-rail-width": "--rintawa-web-activity-rail-width",
  "web.workbench.dock-min-width": "--rintawa-web-dock-min-width",
  "web.workbench.tab-height": "--rintawa-web-tab-height",
  "web.motion.fast": "--rintawa-web-motion-fast",
  "web.motion.normal": "--rintawa-web-motion-normal",
  "web.motion.easing": "--rintawa-web-motion-easing",
  "web.color-scheme": "--rintawa-web-color-scheme",
};

export function themeCssVariables(pack: WebExperiencePack): Record<string, string> {
  const variables: Record<string, string> = {};
  for (const [token, value] of Object.entries(pack.theme.tokens)) {
    const variable = TOKEN_TO_CSS_VARIABLE[token];
    if (variable !== undefined) variables[variable] = value;
  }
  return variables;
}
