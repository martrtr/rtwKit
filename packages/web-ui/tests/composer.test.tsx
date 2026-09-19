import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";

import type { UiPlacementHint, UiPresentationSurface } from "../src/bridge/types";
import {
  STANDARD_EXPERIENCE_PACK,
  mergeExperiencePack,
} from "../src/experience";
import {
  SurfaceComposer,
  groupSurfacesByRegion,
} from "../src/renderer/SurfaceComposer";
import { deriveActivities } from "../src/renderer/WorkbenchComposer";

function surface(
  placement: UiPlacementHint,
  id: string,
  semantic: string | null = null,
  options: {
    instanceId?: string;
    activity?: { id: string; label: string; icon_slot: string | null } | null;
    traits?: string[];
  } = {},
): UiPresentationSurface {
  const semanticParts = semantic?.match(/^(.+)@([0-9]+)$/);
  return {
    owner: {
      instance_id: options.instanceId ?? `demo.${id}`,
      component_id: "runtime",
    },
    contribution: {
      id,
      placement,
      semantic: semanticParts
        ? { id: semanticParts[1], version: Number(semanticParts[2]) }
        : null,
      activity: options.activity ?? null,
      traits: options.traits ?? [],
      required_capabilities: [],
    },
    snapshot: {
      surface_id: id,
      revision: "1",
      root: "root",
      nodes: [{ id: "root", kind: { type: "text", data: { text: id } } }],
    },
  };
}

describe("surface composer", () => {
  test("maps default placement hints through the standard shell profile", () => {
    const surfaces = [
      surface("sidebar", "sidebar-a"),
      surface("primary", "primary-a"),
      surface("sidebar", "sidebar-b"),
      surface("status", "status-a"),
    ];
    const regions = groupSurfacesByRegion(surfaces, STANDARD_EXPERIENCE_PACK);

    expect(regions.get("main")?.map((item) => item.contribution.id)).toEqual(["primary-a"]);
    expect(regions.get("left-dock")?.map((item) => item.contribution.id)).toEqual([
      "sidebar-a",
      "sidebar-b",
    ]);
    expect(regions.get("status")?.map((item) => item.contribution.id)).toEqual(["status-a"]);
  });

  test("renders shell topology and floating layers from the experience pack", () => {
    const markup = renderToStaticMarkup(
      <SurfaceComposer
        surfaces={[
          surface("primary", "primary"),
          surface("sidebar", "sidebar"),
          surface("status", "status"),
          surface("dialog", "dialog"),
          surface("overlay", "overlay"),
        ]}
        onAction={() => undefined}
      />,
    );

    expect(markup).toContain('data-experience-pack="rintawa.web.standard"');
    expect(markup).toContain('data-axis="horizontal"');
    for (const region of ["main", "left-dock", "status", "dialog", "overlay"]) {
      expect(markup).toContain(`data-shell-region="${region}"`);
    }
    expect(markup).toContain('class="rintawa-activity-rail"');
    expect(markup).toContain('class="rintawa-region-resize-handle"');
    expect(markup).toContain('data-presentation="dialog"');
    expect(markup).toContain('data-presentation="overlay"');
  });

  test("trait routing overrides generic placement but not exact semantic", () => {
    const inspector = surface("primary", "inspector", null, {
      traits: ["inspector"],
    });
    const extensions = surface("sidebar", "extensions", "management.extensions@1", {
      traits: ["navigation"],
    });

    const regions = groupSurfacesByRegion(
      [inspector, extensions],
      STANDARD_EXPERIENCE_PACK,
    );

    expect(regions.get("right-dock")?.[0]?.contribution.id).toBe("inspector");
    expect(regions.get("main")?.[0]?.contribution.id).toBe("extensions");
  });

  test("unknown semantics remain reachable through fallback activity region", () => {
    const unknown = surface("primary", "mystery", "future.unknown-panel@1", {
      activity: {
        id: "mystery",
        label: "Mystery",
        icon_slot: "activity.mystery",
      },
    });

    const activities = deriveActivities([unknown], STANDARD_EXPERIENCE_PACK);
    expect(activities).toHaveLength(1);
    expect(activities[0]?.defaultRegion).toBe("main");
    expect(activities[0]?.label).toBe("Mystery");
  });

  test("groups multiple surfaces from one extension under one activity id", () => {
    const activity = {
      id: "world",
      label: "World",
      icon_slot: "activity.world",
    };
    const first = surface("primary", "world.main", null, {
      instanceId: "demo.world",
      activity,
    });
    const second = surface("secondary", "world.detail", null, {
      instanceId: "demo.world",
      activity,
    });

    const activities = deriveActivities(
      [first, second],
      STANDARD_EXPERIENCE_PACK,
    );
    expect(activities).toHaveLength(1);
    expect(activities[0]?.surfaces.map((item) => item.contribution.id)).toEqual([
      "world.main",
      "world.detail",
    ]);
  });

  test("semantic rules override generic placement fallback", () => {
    const visualNovel = mergeExperiencePack(STANDARD_EXPERIENCE_PACK, {
      format: "rintawa.web.experience-pack@1",
      id: "demo.visual-novel",
      name: "Visual Novel",
      extends: "rintawa.web.standard",
      shell: {
        workspace: {
          type: "split",
          axis: "vertical",
          children: [
            { type: "region", region: "scene" },
            { type: "region", region: "dialogue" },
          ],
        },
        layers: [{ region: "spell-overlay", presentation: "overlay" }],
        fallback_region: "scene",
        rules_mode: "replace",
        rules: [
          { match: { placement: "primary" }, region: "scene" },
          { match: { placement: "overlay" }, region: "scene" },
          { match: { semantic: "game.dialogue@1" }, region: "dialogue" },
          { match: { semantic: "game.spell-circle@1" }, region: "spell-overlay" },
        ],
      },
    });

    const genericOverlay = surface("overlay", "generic-overlay");
    const spell = surface("overlay", "spell", "game.spell-circle@1");
    const regions = groupSurfacesByRegion([genericOverlay, spell], visualNovel);

    expect(regions.get("scene")?.map((item) => item.contribution.id)).toEqual([
      "generic-overlay",
    ]);
    expect(regions.get("spell-overlay")?.map((item) => item.contribution.id)).toEqual([
      "spell",
    ]);
  });
});
