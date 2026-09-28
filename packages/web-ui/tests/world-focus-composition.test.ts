import { describe, expect, test } from "vitest";

import type { UiLayerPresentationState, UiPresentationSurface } from "../src/bridge/types";
import { STANDARD_EXPERIENCE_PACK } from "../src/experience";
import {
  deriveFocusedWorldActivities,
  deriveGlobalActivities,
  focusedWorldEntryActivity,
} from "../src/renderer/WorkbenchComposer";

function activitySurface(
  id: string,
  context: UiPresentationSurface["context"],
  hasActivity = true,
): UiPresentationSurface {
  return {
    owner: { instance_id: `demo.${id}`, component_id: "runtime" },
    context,
    contribution: {
      id,
      placement: "primary",
      semantic: null,
      activity: hasActivity
        ? {
            id,
            label: id === "worlds" ? "Worlds" : "Chat",
            icon_slot: `activity.${id}`,
          }
        : null,
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

function presentation(entrySurfaceId: string): UiLayerPresentationState {
  return {
    focused_world: {
      world_id: "world-a",
      descriptor: {
        entry_surface_id: entrySurfaceId,
        presentation_intent: null,
      },
    },
    pending_world_id: null,
    last_focus_error: null,
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

  test("does not invent world-local navigation for surfaces without activities", () => {
    const chatEntry = activitySurface(
      "chat.entry",
      { kind: "focused-world", world_id: "world-a" },
      false,
    );
    const inspector = activitySurface(
      "chat.inspector",
      { kind: "focused-world", world_id: "world-a" },
      false,
    );

    expect(
      deriveFocusedWorldActivities(
        [chatEntry, inspector],
        STANDARD_EXPERIENCE_PACK,
      ),
    ).toEqual([]);
  });

  test("creates one transient workspace entry for the declared world entry surface", () => {
    const chatEntry = activitySurface(
      "chat.entry",
      { kind: "focused-world", world_id: "world-a" },
      false,
    );
    const inspector = activitySurface(
      "chat.inspector",
      { kind: "focused-world", world_id: "world-a" },
      false,
    );

    const entry = focusedWorldEntryActivity(
      [chatEntry, inspector],
      [],
      presentation("chat.entry"),
      STANDARD_EXPERIENCE_PACK,
    );

    expect(entry?.surfaces.map((surface) => surface.contribution.id)).toEqual([
      "chat.entry",
    ]);
    expect(entry?.id).toBe("world-entry:world-a");
  });

  test("reuses the explicit world-local activity when it owns the entry surface", () => {
    const chat = activitySurface("chat.entry", {
      kind: "focused-world",
      world_id: "world-a",
    });
    const activities = deriveFocusedWorldActivities(
      [chat],
      STANDARD_EXPERIENCE_PACK,
    );

    const entry = focusedWorldEntryActivity(
      [chat],
      activities,
      presentation("chat.entry"),
      STANDARD_EXPERIENCE_PACK,
    );

    expect(entry).toBe(activities[0]);
  });
});
