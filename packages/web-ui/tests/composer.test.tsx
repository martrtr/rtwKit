import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";

import type { UiPlacementHint, UiPresentationSurface } from "../src/bridge/types";
import { SurfaceComposer, groupSurfacesByPlacement } from "../src/renderer/SurfaceComposer";

function surface(placement: UiPlacementHint, id: string): UiPresentationSurface {
  return {
    owner: { instance_id: `demo.${id}`, component_id: "runtime" },
    contribution: { id, placement, semantic: null, required_capabilities: [] },
    snapshot: {
      surface_id: id,
      revision: "1",
      root: "root",
      nodes: [{ id: "root", kind: { type: "text", data: { text: id } } }],
    },
  };
}

describe("surface composer", () => {
  test("groups surfaces by placement without changing their order", () => {
    const surfaces = [
      surface("sidebar", "sidebar-a"),
      surface("primary", "primary-a"),
      surface("sidebar", "sidebar-b"),
      surface("status", "status-a"),
    ];
    const groups = groupSurfacesByPlacement(surfaces);

    expect(groups.primary.map((item) => item.contribution.id)).toEqual(["primary-a"]);
    expect(groups.sidebar.map((item) => item.contribution.id)).toEqual([
      "sidebar-a",
      "sidebar-b",
    ]);
    expect(groups.status.map((item) => item.contribution.id)).toEqual(["status-a"]);
    expect(groups.dialog).toEqual([]);
    expect(surfaces.map((item) => item.contribution.id)).toEqual([
      "sidebar-a",
      "primary-a",
      "sidebar-b",
      "status-a",
    ]);
  });

  test("maps portable placement hints to distinct web regions", () => {
    const markup = renderToStaticMarkup(
      <SurfaceComposer
        surfaces={[
          surface("primary", "primary"),
          surface("secondary", "secondary"),
          surface("sidebar", "sidebar"),
          surface("settings", "settings"),
          surface("status", "status"),
          surface("dialog", "dialog"),
          surface("overlay", "overlay"),
        ]}
        onAction={() => undefined}
      />,
    );

    const placements = [
      "primary",
      "secondary",
      "sidebar",
      "settings",
      "status",
      "dialog",
      "overlay",
    ];
    for (const placement of placements) {
      expect(markup).toContain(`data-placement-group="${placement}"`);
    }
    expect(markup).toContain(`class="rintawa-floating-layer"`);
    expect(markup).toContain(`data-layout="split"`);
  });

  test("uses the full workspace width when only a sidebar is mounted", () => {
    const markup = renderToStaticMarkup(
      <SurfaceComposer
        surfaces={[surface("sidebar", "sidebar")]}
        onAction={() => undefined}
      />,
    );

    expect(markup).toContain(`data-layout="single"`);
    expect(markup).not.toContain(`class="rintawa-main-stack"`);
  });
});
