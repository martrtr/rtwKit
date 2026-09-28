import { describe, expect, test } from "vitest";

import type { UiPresentationSurface } from "../src/bridge/types";
import { STANDARD_EXPERIENCE_PACK } from "../src/experience";
import { deriveGlobalActivities } from "../src/renderer/WorkbenchComposer";

function activitySurface(
  id: string,
  context: UiPresentationSurface["context"],
): UiPresentationSurface {
  return {
    owner: { instance_id: `demo.${id}`, component_id: "runtime" },
    context,
    contribution: {
      id,
      placement: "primary",
      semantic: null,
      activity: {
        id,
        label: id === "worlds" ? "Worlds" : "Chat",
        icon_slot: `activity.${id}`,
      },
      traits: [],
      required_capabilities: [],
    },
    snapshot: {
      surface_id: id,
      revision: "1",
      root: "root",
      nodes: [
        { id: "root", kind: { type: "text", data: { text: id } } },
      ],
    },
  };
}

describe("focused world composition", () => {
  test("keeps world-local activities out of the global launcher", () => {
    const worlds = activitySurface("worlds", { kind: "layer-local" });
    const chat = activitySurface("chat", {
      kind: "focused-world",
      world_id: "world-a",
    });

    const activities = deriveGlobalActivities(
      [worlds, chat],
      STANDARD_EXPERIENCE_PACK,
    );

    expect(activities.map((activity) => activity.id)).toEqual(["worlds"]);
  });
});
