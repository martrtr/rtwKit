import { z } from "zod";

import {
  WEB_EXPERIENCE_MODULE_FORMAT,
  WEB_EXPERIENCE_PACK_FORMAT,
} from "./types";
import type {
  ShellLayoutNode,
  WebExperienceModule,
  WebExperiencePack,
  WebExperiencePackPatch,
} from "./types";

const identifier = z.string().min(1).max(128);
const placement = z.enum([
  "primary",
  "secondary",
  "sidebar",
  "settings",
  "dialog",
  "status",
  "overlay",
]);

const layoutNode: z.ZodType<ShellLayoutNode> = z.lazy(() =>
  z.discriminatedUnion("type", [
    z.object({
      type: z.literal("region"),
      region: identifier,
      weight: z.number().positive().max(100).optional(),
    }),
    z.object({
      type: z.literal("split"),
      axis: z.enum(["horizontal", "vertical"]),
      children: z.array(layoutNode).min(1).max(16),
    }),
  ]),
);

const layer = z.object({
  region: identifier,
  presentation: z.enum(["status", "dialog", "overlay"]),
});

const rule = z.object({
  match: z
    .object({
      semantic: identifier.optional(),
      trait: identifier.optional(),
      placement: placement.optional(),
    })
    .refine(
      (value) =>
        value.semantic !== undefined ||
        value.trait !== undefined ||
        value.placement !== undefined,
      "shell rule requires semantic, trait and/or placement",
    ),
  region: identifier,
});

const regionResize = z
  .object({
    edge: z.enum(["start", "end"]),
    min_size: z.number().int().min(48).max(4096).optional(),
    max_size: z.number().int().min(48).max(4096).optional(),
  })
  .refine(
    (value) =>
      value.min_size === undefined ||
      value.max_size === undefined ||
      value.min_size <= value.max_size,
    "resize min_size must not exceed max_size",
  );

const regionPresentation = z.object({
  region: identifier,
  label: z.string().min(1).max(128),
  mode: z.enum(["plain", "activity-tabs"]),
  accepts_activities: z.boolean().optional(),
  collapsible: z.boolean().optional(),
  resize: regionResize.optional(),
});

const activityBar = z.object({
  presentation: z.enum([
    "vertical-start",
    "vertical-end",
    "horizontal-top",
    "hidden",
  ]),
});

const stringMap = z.record(z.string().min(1).max(128), z.string().min(1).max(512));

const theme = z.object({
  tokens: stringMap,
  icons: stringMap.optional(),
});

const shell = z.object({
  workspace: layoutNode,
  layers: z.array(layer).max(32),
  rules: z.array(rule).max(256),
  fallback_region: identifier,
  region_presentations: z.array(regionPresentation).max(64).optional(),
  activity_bar: activityBar.optional(),
});

const fullPack = z.object({
  format: z.literal(WEB_EXPERIENCE_PACK_FORMAT),
  id: identifier,
  name: z.string().min(1).max(128),
  theme,
  shell,
});

const themePatch = z
  .object({
    tokens: stringMap.optional(),
    icons: stringMap.optional(),
  })
  .optional();

const shellPatch = z
  .object({
    workspace: layoutNode.optional(),
    layers: z.array(layer).max(32).optional(),
    rules: z.array(rule).max(256).optional(),
    rules_mode: z.enum(["prepend", "replace"]).optional(),
    fallback_region: identifier.optional(),
    region_presentations: z.array(regionPresentation).max(64).optional(),
    activity_bar: activityBar.optional(),
  })
  .optional();

const patchPack = z.object({
  format: z.literal(WEB_EXPERIENCE_PACK_FORMAT),
  id: identifier,
  name: z.string().min(1).max(128),
  extends: identifier,
  theme: themePatch,
  shell: shellPatch,
});

const experienceModule = z.object({
  format: z.literal(WEB_EXPERIENCE_MODULE_FORMAT),
  id: identifier,
  name: z.string().min(1).max(128),
  targets: z.array(identifier).min(1).max(32).optional(),
  theme: themePatch,
  shell: shellPatch,
});

export function parseExperiencePack(value: unknown): WebExperiencePack {
  return validateExperiencePack(fullPack.parse(value));
}

export function parseExperiencePackPatch(value: unknown): WebExperiencePackPatch {
  return patchPack.parse(value);
}

export function parseExperienceModule(value: unknown): WebExperienceModule {
  return experienceModule.parse(value);
}

export function validateExperiencePack(pack: WebExperiencePack): WebExperiencePack {
  const regions = new Set<string>();
  collectWorkspaceRegions(pack.shell.workspace, regions);

  for (const item of pack.shell.layers) {
    if (regions.has(item.region)) {
      throw new Error(`region '${item.region}' cannot be both workspace and floating layer`);
    }
    regions.add(item.region);
  }

  if (!regions.has(pack.shell.fallback_region)) {
    throw new Error(`fallback region '${pack.shell.fallback_region}' is not declared`);
  }

  for (const rule of pack.shell.rules) {
    if (!regions.has(rule.region)) {
      throw new Error(`shell rule references unknown region '${rule.region}'`);
    }
  }

  const presentations = new Set<string>();
  for (const presentation of pack.shell.region_presentations ?? []) {
    if (!regions.has(presentation.region)) {
      throw new Error(
        `region presentation references unknown region '${presentation.region}'`,
      );
    }
    if (presentations.has(presentation.region)) {
      throw new Error(`duplicate region presentation '${presentation.region}'`);
    }
    presentations.add(presentation.region);
  }

  if (
    pack.shell.activity_bar !== undefined &&
    pack.shell.activity_bar.presentation !== "hidden"
  ) {
    const fallback = (pack.shell.region_presentations ?? []).find(
      (presentation) => presentation.region === pack.shell.fallback_region,
    );
    if (!fallback?.accepts_activities) {
      throw new Error(
        "activity-enabled shell requires fallback_region to accept activities",
      );
    }
  }

  return pack;
}

function collectWorkspaceRegions(node: ShellLayoutNode, regions: Set<string>): void {
  if (node.type === "region") {
    if (regions.has(node.region)) {
      throw new Error(`duplicate shell region '${node.region}'`);
    }
    regions.add(node.region);
    return;
  }

  for (const child of node.children) {
    collectWorkspaceRegions(child, regions);
  }
}
