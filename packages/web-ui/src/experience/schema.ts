import { z } from "zod";

import { WEB_EXPERIENCE_PACK_FORMAT } from "./types";
import type {
  ShellLayoutNode,
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
      placement: placement.optional(),
    })
    .refine(
      (value) => value.semantic !== undefined || value.placement !== undefined,
      "shell rule requires semantic and/or placement",
    ),
  region: identifier,
});

const theme = z.object({
  tokens: z.record(z.string().min(1), z.string().max(512)),
});

const shell = z.object({
  workspace: layoutNode,
  layers: z.array(layer).max(32),
  rules: z.array(rule).max(256),
  fallback_region: identifier,
});

const fullPack = z.object({
  format: z.literal(WEB_EXPERIENCE_PACK_FORMAT),
  id: identifier,
  name: z.string().min(1).max(128),
  theme,
  shell,
});

const patchPack = z.object({
  format: z.literal(WEB_EXPERIENCE_PACK_FORMAT),
  id: identifier,
  name: z.string().min(1).max(128),
  extends: identifier,
  theme: z
    .object({
      tokens: z.record(z.string().min(1), z.string().max(512)).optional(),
    })
    .optional(),
  shell: z
    .object({
      workspace: layoutNode.optional(),
      layers: z.array(layer).max(32).optional(),
      rules: z.array(rule).max(256).optional(),
      rules_mode: z.enum(["prepend", "replace"]).optional(),
      fallback_region: identifier.optional(),
    })
    .optional(),
});

export function parseExperiencePack(value: unknown): WebExperiencePack {
  return validateExperiencePack(fullPack.parse(value));
}

export function parseExperiencePackPatch(value: unknown): WebExperiencePackPatch {
  return patchPack.parse(value);
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
