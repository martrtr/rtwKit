import { readFile } from "node:fs/promises";

import { describe, expect, test } from "vitest";

import { DemoUiTransport } from "../src/bridge/demo";
import { sameOriginWebSocketUrl } from "../src/bridge";
import { parseHostMessage } from "../src/bridge/schema";
import {
  PORTABLE_UI_PROTOCOL_MAJOR,
  UI_CAPABILITIES,
  WEB_UI_BRIDGE_PROTOCOL_MAJOR,
} from "../src/bridge/types";

describe("web UI bridge", () => {

  test("derives the production same-origin WebSocket endpoint", () => {
    expect(sameOriginWebSocketUrl("http:", "127.0.0.1:3000")).toBe(
      "ws://127.0.0.1:3000/__rintawa/ws",
    );
    expect(sameOriginWebSocketUrl("https:", "rintawa.example")).toBe(
      "wss://rintawa.example/__rintawa/ws",
    );
    expect(sameOriginWebSocketUrl("file:", "")).toBeNull();
  });
  test("accepts exact decimal string revisions", () => {
    const message = parseHostMessage({
      type: "state",
      protocol_major: 1,
      surfaces: [
        {
          owner: { instance_id: "feature", component_id: "runtime" },
          contribution: {
            id: "main",
            placement: "primary",
            semantic: null,
            activity: null,
      traits: [],
      required_capabilities: [],
          },
          snapshot: {
            surface_id: "main",
            revision: "18446744073709551615",
            root: "root",
            nodes: [{ id: "root", kind: { type: "text", data: { text: "hello" } } }],
          },
        },
      ],
    });

    expect(message.type).toBe("state");
  });


  test("defaults omitted button appearance for older surfaces", () => {
    const message = parseHostMessage({
      type: "state",
      protocol_major: 1,
      surfaces: [
        {
          owner: { instance_id: "feature", component_id: "runtime" },
          contribution: {
            id: "main",
            placement: "primary",
            semantic: null,
            activity: null,
            traits: [],
            required_capabilities: ["rintawa.ui.button@1"],
          },
          snapshot: {
            surface_id: "main",
            revision: "1",
            root: "button",
            nodes: [
              {
                id: "button",
                kind: {
                  type: "button",
                  data: {
                    label: "Run",
                    action: "demo.run",
                    is_enabled: true,
                  },
                },
              },
            ],
          },
        },
      ],
    });

    expect(message.type).toBe("state");
    if (message.type === "state") {
      const button = message.surfaces[0]?.snapshot.nodes[0];
      expect(button?.kind.type).toBe("button");
      if (button?.kind.type === "button") {
        expect(button.kind.data.appearance).toBe("default");
        expect(button.semantic).toBeNull();
        expect(button.traits).toEqual([]);
      }
    }
  });

  test("accepts renderer-neutral data grids", () => {
    const message = parseHostMessage({
      type: "state",
      protocol_major: 1,
      surfaces: [
        {
          owner: { instance_id: "feature", component_id: "runtime" },
          contribution: {
            id: "main",
            placement: "primary",
            semantic: null,
            activity: null,
            traits: [],
            required_capabilities: ["rintawa.ui.data-grid@1"],
          },
          snapshot: {
            surface_id: "main",
            revision: "1",
            root: "grid",
            nodes: [
              {
                id: "grid",
                kind: {
                  type: "data-grid",
                  data: {
                    columns: [
                      {
                        key: "name",
                        label: "Name",
                        weight: 3,
                        sort_action: "grid.sort",
                        sort_direction: "ascending",
                      },
                      {
                        key: "status",
                        label: "Status",
                        weight: 1,
                        sort_action: "grid.sort",
                        sort_direction: null,
                      },
                    ],
                    cells: ["name", "status"],
                    selected_rows: [0],
                    row_keys: ["demo"],
                    row_action: "grid.activate-row",
                  },
                },
              },
              { id: "name", kind: { type: "text", data: { text: "Demo" } } },
              { id: "status", kind: { type: "text", data: { text: "Ready" } } },
            ],
          },
        },
      ],
    });

    expect(message.type).toBe("state");
    if (message.type === "state") {
      const grid = message.surfaces[0]?.snapshot.nodes[0];
      expect(grid?.kind.type).toBe("data-grid");
      if (grid?.kind.type === "data-grid") {
        expect(grid.kind.data.selected_rows).toEqual([0]);
        expect(grid.kind.data.row_keys).toEqual(["demo"]);
        expect(grid.kind.data.row_action).toBe("grid.activate-row");
        expect(grid.kind.data.columns[0]?.key).toBe("name");
        expect(grid.kind.data.columns[0]?.sort_direction).toBe("ascending");
      }
    }
  });

  test("defaults omitted data-grid selection for older surfaces", () => {
    const message = parseHostMessage({
      type: "state",
      protocol_major: 1,
      surfaces: [
        {
          owner: { instance_id: "feature", component_id: "runtime" },
          contribution: {
            id: "main",
            placement: "primary",
            semantic: null,
            activity: null,
            traits: [],
            required_capabilities: ["rintawa.ui.data-grid@1"],
          },
          snapshot: {
            surface_id: "main",
            revision: "1",
            root: "grid",
            nodes: [
              {
                id: "grid",
                kind: {
                  type: "data-grid",
                  data: {
                    columns: [{ label: "Name", weight: 1 }],
                    cells: ["name"],
                  },
                },
              },
              { id: "name", kind: { type: "text", data: { text: "Demo" } } },
            ],
          },
        },
      ],
    });

    expect(message.type).toBe("state");
    if (message.type === "state") {
      const grid = message.surfaces[0]?.snapshot.nodes[0];
      if (grid?.kind.type === "data-grid") {
        expect(grid.kind.data.selected_rows).toEqual([]);
        expect(grid.kind.data.row_keys).toEqual([]);
        expect(grid.kind.data.row_action).toBeNull();
        expect(grid.kind.data.columns[0]?.key).toBeNull();
        expect(grid.kind.data.columns[0]?.sort_action).toBeNull();
        expect(grid.kind.data.columns[0]?.sort_direction).toBeNull();
      }
    }
  });

  test("rejects data-grid selection outside the row range", () => {
    expect(() =>
      parseHostMessage({
        type: "state",
        protocol_major: 1,
        surfaces: [
          {
            owner: { instance_id: "feature", component_id: "runtime" },
            contribution: {
              id: "main",
              placement: "primary",
              semantic: null,
              activity: null,
              traits: [],
              required_capabilities: ["rintawa.ui.data-grid@1"],
            },
            snapshot: {
              surface_id: "main",
              revision: "1",
              root: "grid",
              nodes: [
                {
                  id: "grid",
                  kind: {
                    type: "data-grid",
                    data: {
                      columns: [{ label: "Name", weight: 1 }],
                      cells: ["name"],
                      selected_rows: [1],
                    },
                  },
                },
                { id: "name", kind: { type: "text", data: { text: "Demo" } } },
              ],
            },
          },
        ],
      }),
    ).toThrow();
  });

  test("rejects invalid revisions at the web boundary", () => {
    const message = (revision: unknown) => ({
      type: "state",
      protocol_major: 1,
      surfaces: [
        {
          owner: { instance_id: "feature", component_id: "runtime" },
          contribution: {
            id: "main",
            placement: "primary",
            semantic: null,
            activity: null,
      traits: [],
      required_capabilities: [],
          },
          snapshot: {
            surface_id: "main",
            revision,
            root: "root",
            nodes: [],
          },
        },
      ],
    });

    expect(() => parseHostMessage(message(Number.MAX_SAFE_INTEGER + 1))).toThrow();
    expect(() => parseHostMessage(message("18446744073709551616"))).toThrow();
  });

  test("keeps packaged capabilities and versions in sync", async () => {
    const descriptor = await readFile(new URL("../rtw/web-layer.toml", import.meta.url), "utf8");
    const capabilityBlock = descriptor.match(/capabilities = \[([\s\S]*?)\]/)?.[1] ?? "";
    const packagedCapabilities = [...capabilityBlock.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
    expect(packagedCapabilities).toEqual([...UI_CAPABILITIES]);

    const packageJson = JSON.parse(
      await readFile(new URL("../package.json", import.meta.url), "utf8"),
    ) as { version: string };
    const rtwkit = await readFile(new URL("../rtwkit.toml", import.meta.url), "utf8");
    const manifest = await readFile(new URL("../rtw/manifest.toml", import.meta.url), "utf8");
    const versionOf = (source: string) => source.match(/^version = "([^"]+)"$/m)?.[1];

    expect(versionOf(rtwkit)).toBe(packageJson.version);
    expect(versionOf(manifest)).toBe(packageJson.version);
    expect(descriptor).toContain(`bridge-protocol-major = ${WEB_UI_BRIDGE_PROTOCOL_MAJOR}`);
    expect(descriptor).toContain(`protocol-major = ${PORTABLE_UI_PROTOCOL_MAJOR}`);
  });

  test("demo transport completes hello and action flow", async () => {
    const transport = new DemoUiTransport();
    const states: string[] = [];
    const disconnect = await transport.connect(
      (message) => {
        if (message.type === "state") {
          states.push(message.surfaces[0]?.snapshot.revision ?? "missing");
        }
      },
      (error) => {
        throw error;
      },
    );

    await transport.send({
      type: "hello",
      protocol_major: WEB_UI_BRIDGE_PROTOCOL_MAJOR,
      portable_ui_protocol_major: PORTABLE_UI_PROTOCOL_MAJOR,
      capabilities: [...UI_CAPABILITIES],
    });
    await Promise.resolve();
    await transport.send({
      type: "action",
      protocol_major: WEB_UI_BRIDGE_PROTOCOL_MAJOR,
      event: {
        owner_instance_id: "demo.feature",
        surface_id: "demo.main",
        node_id: "button",
        action_id: "demo.run",
        surface_revision: "1",
        payload: { type: "none" },
      },
    });
    await Promise.resolve();

    expect(states).toEqual(["1", "2"]);
    disconnect();
  });
});
