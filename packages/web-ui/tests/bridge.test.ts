import { readFile } from "node:fs/promises";

import { describe, expect, test } from "vitest";

import { DemoUiTransport } from "../src/bridge/demo";
import { parseHostMessage } from "../src/bridge/schema";
import {
  PORTABLE_UI_PROTOCOL_MAJOR,
  UI_CAPABILITIES,
  WEB_UI_BRIDGE_PROTOCOL_MAJOR,
} from "../src/bridge/types";

describe("web UI bridge", () => {
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
