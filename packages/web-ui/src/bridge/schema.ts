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

const activity = z.object({
  id: identifier,
  label: z.string(),
  icon_slot: identifier.nullable(),
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
  activity: activity.nullable(),
  traits: z.array(identifier),
  required_capabilities: z.array(identifier),
});

const icon = z.object({
  slot: identifier,
  label: z.string().nullable(),
  size: z.number().int().positive().max(256).nullable(),
});

const image = z.object({
  media_type: z.enum(["image/png", "image/webp"]),
  data_base64: z.string().max(1024 * 1024),
  alt: z.string(),
  width: z.number().int().positive().max(4096).nullable(),
  height: z.number().int().positive().max(4096).nullable(),
});

const checkbox = z.object({
  label: z.string(),
  checked: z.boolean(),
  change_action: identifier,
  is_enabled: z.boolean(),
});

const select = z.object({
  value: z.string(),
  options: z
    .array(z.object({ value: z.string(), label: z.string() }))
    .max(256),
  change_action: identifier,
  is_enabled: z.boolean(),
});

const split = z
  .object({
    children: z.array(identifier),
    weights: z.array(z.number().int().min(1).max(10_000)),
    axis: z.enum(["horizontal", "vertical"]),
  })
  .refine(
    (value) => value.children.length === value.weights.length,
    "split children/weights length mismatch",
  );

const textInput = z.object({
  value: z.string(),
  placeholder: z.string().nullable(),
  change_action: identifier.nullable(),
  submit_action: identifier.nullable(),
  is_enabled: z.boolean(),
});

const dataGrid = z
  .object({
    columns: z
      .array(
        z.object({
          key: identifier.nullable().default(null),
          label: z.string(),
          weight: z.number().int().min(1).max(10_000),
          sort_action: identifier.nullable().default(null),
          sort_direction: z.enum(["ascending", "descending"]).nullable().default(null),
        }),
      )
      .min(1)
      .max(64),
    cells: z.array(identifier),
    selected_rows: z.array(z.number().int().min(0)).max(4096).default([]),
    row_keys: z.array(identifier).max(4096).default([]),
    row_action: identifier.nullable().default(null),
  })
  .superRefine((value, context) => {
    if (value.cells.length % value.columns.length !== 0) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: "data-grid cell count must be divisible by column count",
      });
      return;
    }

    const sortableKeys = new Set<string>();
    for (const column of value.columns) {
      if (column.key !== null && sortableKeys.has(column.key)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          message: "data-grid column keys must be unique",
        });
        return;
      }
      if (column.key !== null) sortableKeys.add(column.key);
      if (column.sort_action !== null && column.key === null) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          message: "sortable data-grid columns require stable keys",
        });
        return;
      }
      if (column.sort_direction !== null && column.sort_action === null) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          message: "data-grid sort direction requires a sort action",
        });
        return;
      }
    }

    const rowCount = value.cells.length / value.columns.length;
    if (value.row_action !== null && value.row_keys.length !== rowCount) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: "interactive data-grid rows require one row key per row",
      });
      return;
    }
    if (value.row_keys.length > 0) {
      if (value.row_keys.length !== rowCount || new Set(value.row_keys).size !== value.row_keys.length) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          message: "data-grid row keys must be unique and match the row count",
        });
        return;
      }
    }

    const selected = new Set<number>();
    for (const row of value.selected_rows) {
      if (row >= rowCount || selected.has(row)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          message: "data-grid contains an invalid selected row",
        });
        return;
      }
      selected.add(row);
    }
  });

const nodeKind = z.discriminatedUnion("type", [
  z.object({ type: z.literal("text"), data: z.object({ text: z.string() }) }),
  z.object({ type: z.literal("markdown"), data: z.object({ source: z.string() }) }),
  z.object({
    type: z.literal("button"),
    data: z.object({
      label: z.string(),
      action: identifier,
      is_enabled: z.boolean(),
      appearance: z
        .enum(["default", "primary", "subtle", "danger"])
        .default("default"),
    }),
  }),
  z.object({ type: z.literal("icon"), data: icon }),
  z.object({ type: z.literal("image"), data: image }),
  z.object({ type: z.literal("checkbox"), data: checkbox }),
  z.object({ type: z.literal("select"), data: select }),
  z.object({ type: z.literal("text-input"), data: textInput }),
  z.object({ type: z.literal("text-area"), data: textInput }),
  z.object({ type: z.literal("split"), data: split }),
  z.object({ type: z.literal("row"), data: z.object({ children: z.array(identifier) }) }),
  z.object({ type: z.literal("column"), data: z.object({ children: z.array(identifier) }) }),
  z.object({ type: z.literal("list"), data: z.object({ children: z.array(identifier) }) }),
  z.object({ type: z.literal("data-grid"), data: dataGrid }),
]);

const surface = z.object({
  owner: componentRef,
  contribution,
  snapshot: z.object({
    surface_id: identifier,
    revision,
    root: identifier,
    nodes: z.array(
      z.object({
        id: identifier,
        semantic: contractKey.nullable().default(null),
        traits: z.array(identifier).default([]),
        kind: nodeKind,
      }),
    ),
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
