import { afterEach, describe, expect, test, vi } from "vitest";

import type {
  ShellLayoutNode,
  WebExperiencePack,
} from "../src/experience/types";
import {
  EMPTY_WORKSPACE_PREFERENCES,
  readWorkspacePreferences,
  shellLayoutIdentity,
  shellLayoutLabel,
  shellResizeBounds,
  writeWorkspacePreferences,
} from "../src/renderer/workspaceLayout";

const workspace: ShellLayoutNode = {
  type: "split",
  axis: "horizontal",
  children: [
    { type: "region", region: "left", weight: 0.3 },
    { type: "region", region: "main", weight: 1 },
  ],
};

const pack: WebExperiencePack = {
  format: "rintawa.web.experience-pack@1",
  id: "test.workspace",
  name: "Test Workspace",
  theme: { tokens: {} },
  shell: {
    workspace,
    layers: [],
    rules: [],
    fallback_region: "main",
    region_presentations: [
      {
        region: "left",
        label: "Navigation",
        mode: "plain",
        resize: { edge: "end", min_size: 180, max_size: 720 },
      },
      {
        region: "main",
        label: "Workspace",
        mode: "plain",
      },
    ],
  },
};

describe("workspace layout policy", () => {
  afterEach(() => vi.unstubAllGlobals());

  test("derives persistent split identity from stable topology rather than labels", () => {
    expect(shellLayoutIdentity(workspace)).toBe(
      "split:horizontal(region:left|region:main)",
    );

    const renamed = structuredClone(pack);
    renamed.shell.region_presentations![0]!.label = "Renamed Navigation";
    expect(shellLayoutIdentity(renamed.shell.workspace)).toBe(
      shellLayoutIdentity(workspace),
    );
  });

  test("persists activity section navigation widths with workspace preferences", () => {
    const values = new Map<string, string>();
    vi.stubGlobal("window", {
      localStorage: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
      },
    });
    const preferences = {
      ...EMPTY_WORKSPACE_PREFERENCES,
      sectionNavigationWidths: { "activity:rintawa.management": 284 },
    };

    writeWorkspacePreferences(pack, preferences);
    expect(readWorkspacePreferences(pack).sectionNavigationWidths).toEqual(
      preferences.sectionNavigationWidths,
    );
  });

  test("uses declared region bounds and safe generic fallbacks", () => {
    const left = workspace.type === "split" ? workspace.children[0]! : workspace;
    const main = workspace.type === "split" ? workspace.children[1]! : workspace;

    expect(shellResizeBounds(pack, left)).toEqual({
      minimum: 180,
      maximum: 720,
    });
    expect(shellResizeBounds(pack, main)).toEqual({
      minimum: 96,
      maximum: Number.POSITIVE_INFINITY,
    });
    expect(shellLayoutLabel(pack, left)).toBe("Navigation");
    expect(shellLayoutLabel(pack, main)).toBe("Workspace");
  });
});
