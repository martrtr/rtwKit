import { z } from "zod";

import type { HostToRendererMessage } from "./types";
import { WEB_UI_BRIDGE_PROTOCOL_MAJOR } from "./types";

const identifier = z.string().min(1);
const U64_MAX = 18_446_744_073_709_551_615n;
const revision = z
  .string()
  .regex(/^(0|[1-9][0-9]*)$/)
  .refine((value) => BigInt(value) <= U64_MAX, "revision exceeds u64 range");

const componentRef = z.object({
  instance_id: identifier,
  component_id: identifier,
});

const contractKey = z.object({
  id: identifier,
  version: z.number().int().min(0).max(4_294_967_295),
});

const contribution = z.object({
  id: identifier,
  placement: z.enum([
    "primary",
    "secondary",
    "sidebar",
    "settings",
    "dialog",
    "status",
    "overlay",
  ]),
  semantic: contractKey.nullable(),
  required_capabilities: z.array(identifier),
});

const textInput = z.object({
  value: z.string(),
  placeholder: z.string().nullable(),
  change_action: identifier.nullable(),
  submit_action: identifier.nullable(),
  is_enabled: z.boolean(),
});

const nodeKind = z.discriminatedUnion("type", [
  z.object({ type: z.literal("text"), data: z.object({ text: z.string() }) }),
  z.object({ type: z.literal("markdown"), data: z.object({ source: z.string() }) }),
  z.object({
    type: z.literal("button"),
    data: z.object({ label: z.string(), action: identifier, is_enabled: z.boolean() }),
  }),
  z.object({ type: z.literal("text-input"), data: textInput }),
  z.object({ type: z.literal("text-area"), data: textInput }),
  z.object({ type: z.literal("row"), data: z.object({ children: z.array(identifier) }) }),
  z.object({ type: z.literal("column"), data: z.object({ children: z.array(identifier) }) }),
  z.object({ type: z.literal("list"), data: z.object({ children: z.array(identifier) }) }),
]);

const surface = z.object({
  owner: componentRef,
  contribution,
  snapshot: z.object({
    surface_id: identifier,
    revision,
    root: identifier,
    nodes: z.array(z.object({ id: identifier, kind: nodeKind })),
  }),
});

const hostMessage = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("state"),
    protocol_major: z.literal(WEB_UI_BRIDGE_PROTOCOL_MAJOR),
    surfaces: z.array(surface),
  }),
  z.object({
    type: z.literal("error"),
    protocol_major: z.literal(WEB_UI_BRIDGE_PROTOCOL_MAJOR),
    code: identifier,
    message: z.string(),
  }),
]);

export function parseHostMessage(value: unknown): HostToRendererMessage {
  return hostMessage.parse(value) as HostToRendererMessage;
}
